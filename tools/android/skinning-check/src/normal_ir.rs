//! Tiny fail-closed CPU evaluator for the actual Naga normal-function IR.
//! Only the operations encountered here are admitted. No GPU or mirrored shader body.
use naga::{BinaryOperator as Op, Expression as E, MathFunction as Math, Statement as S};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    F(f32),
    V(Vec<f32>),
    M(Vec<Vec<f32>>),
    B(bool),
    BV(Vec<bool>),
}
type Result<T> = std::result::Result<T, String>;
pub const DIVIDE_ZERO: &str = "division by zero in actual shader IR";
pub const NORMALIZE_ZERO: &str = "normalize zero in actual shader IR";
fn vector(value: Value) -> Result<Vec<f32>> {
    match value { Value::V(v) => Ok(v), _ => Err("vector required".into()) }
}
fn scalar(value: Value) -> Result<f32> {
    match value { Value::F(v) => Ok(v), _ => Err("scalar required".into()) }
}
fn binary(op: Op, left: Value, right: Value) -> Result<Value> {
    use Value::*;
    match (op, left, right) {
        (Op::Equal, F(a), F(b)) => Ok(B(a == b)),
        (Op::Equal, V(a), V(b)) if a.len() == b.len() => Ok(BV(a.iter().zip(b).map(|(a,b)| *a == b).collect())),
        (Op::Divide, V(a), F(b)) => {
            if b == 0.0 { return Err(DIVIDE_ZERO.into()); }
            Ok(V(a.into_iter().map(|v| v / b).collect()))
        }
        // Naga lowers vector/scalar division to a component-wise vector divide
        // after splatting the scalar. Execute exactly that IR, not a shader copy.
        (Op::Divide, V(a), V(b)) => {
            if a.len() != b.len() { return Err("component-wise divide dimensions differ".into()); }
            if b.contains(&0.0) { return Err(DIVIDE_ZERO.into()); }
            Ok(V(a.into_iter().zip(b).map(|(a,b)| a/b).collect()))
        }
        (Op::Multiply, M(a), V(b)) if a.len() == b.len() => {
            let mut result = vec![0.0; a[0].len()];
            for (column, weight) in a.iter().zip(b) {
                for (result, value) in result.iter_mut().zip(column) { *result += value * weight; }
            }
            Ok(V(result))
        }
        other => Err(format!("unsupported IR binary: {other:?}")),
    }
}

struct Frame<'a> {
    module: &'a naga::Module,
    function: &'a naga::Function,
    args: Vec<Value>,
    values: HashMap<usize, Value>,
}
impl Frame<'_> {
    fn expr(&mut self, handle: naga::Handle<E>) -> Result<Value> {
        if let Some(value) = self.values.get(&handle.index()) { return Ok(value.clone()); }
        let value = match self.function.expressions[handle].clone() {
            E::Literal(naga::Literal::F32(value)) => Value::F(value),
            E::FunctionArgument(index) => self.args[index as usize].clone(),
            E::AccessIndex { base, index } => match self.expr(base)? {
                Value::M(columns) => Value::V(columns[index as usize].clone()),
                Value::V(values) => Value::F(values[index as usize]),
                _ => return Err("unsupported access".into()),
            },
            E::Compose { ty, components } => match self.module.types[ty].inner {
                naga::TypeInner::Vector { .. } => {
                    let mut result = Vec::new();
                    for component in components { result.push(scalar(self.expr(component)?)?); }
                    Value::V(result)
                }
                naga::TypeInner::Matrix { .. } => {
                    let mut result = Vec::new();
                    for component in components { result.push(vector(self.expr(component)?)?); }
                    Value::M(result)
                }
                _ => return Err("unsupported composition".into()),
            },
            E::Splat { size, value } => Value::V(vec![scalar(self.expr(value)?)?; size as usize]),
            E::Swizzle { size, vector: source, pattern } => {
                let source = vector(self.expr(source)?)?;
                Value::V(pattern[..size as usize].iter().map(|axis| source[*axis as usize]).collect())
            }
            E::Binary { op, left, right } => binary(op, self.expr(left)?, self.expr(right)?)?,
            E::Math { fun, arg, arg1, .. } => {
                let a = vector(self.expr(arg)?)?;
                match fun {
                    Math::Cross => {
                        let b = vector(self.expr(arg1.ok_or("missing cross argument")?)?)?;
                        if a.len() != 3 || b.len() != 3 { return Err("cross dimensions".into()); }
                        Value::V(vec![a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]])
                    }
                    Math::Dot => {
                        let b = vector(self.expr(arg1.ok_or("missing dot argument")?)?)?;
                        if a.len() != b.len() { return Err("dot dimensions".into()); }
                        Value::F(a.iter().zip(b).map(|(a,b)| a*b).sum())
                    }
                    Math::Normalize => {
                        let length = a.iter().map(|v| v*v).sum::<f32>().sqrt();
                        if length == 0.0 { return Err(NORMALIZE_ZERO.into()); }
                        if !length.is_finite() { return Err("nonfinite normalization length".into()); }
                        Value::V(a.iter().map(|v| v/length).collect())
                    }
                    _ => return Err(format!("unsupported math: {fun:?}")),
                }
            }
            E::Relational { fun: naga::RelationalFunction::All, argument } => match self.expr(argument)? {
                Value::BV(values) => Value::B(values.into_iter().all(|v| v)),
                _ => return Err("all requires bool vector".into()),
            },
            other => return Err(format!("unsupported actual shader expression: {other:?}")),
        };
        self.values.insert(handle.index(), value.clone());
        Ok(value)
    }
    fn block(&mut self, block: &naga::Block) -> Result<Option<Value>> {
        for statement in block {
            match statement {
                S::Emit(range) => for expression in range.clone() { self.expr(expression)?; },
                S::Return { value } => return value.map(|h| self.expr(h)).transpose(),
                S::If { condition, accept, reject } => {
                    let Value::B(condition) = self.expr(*condition)? else { return Err("boolean condition required".into()); };
                    if let Some(value) = self.block(if condition { accept } else { reject })? { return Ok(Some(value)); }
                }
                S::Block(block) => if let Some(value) = self.block(block)? { return Ok(Some(value)); },
                S::Call { function, arguments, result } => {
                    let mut args = Vec::new();
                    for argument in arguments { args.push(self.expr(*argument)?); }
                    let value = call(self.module, *function, args)?;
                    self.values.insert(result.ok_or("value-returning call required")?.index(), value);
                }
                other => return Err(format!("unsupported actual shader statement: {other:?}")),
            }
        }
        Ok(None)
    }
}
fn call(module: &naga::Module, handle: naga::Handle<naga::Function>, args: Vec<Value>) -> Result<Value> {
    let function = &module.functions[handle];
    let mut frame = Frame { module, function, args, values: HashMap::new() };
    frame.block(&function.body)?.ok_or_else(|| "missing shader return".into())
}
pub fn normal(module: &naga::Module, matrix: [[f32; 4]; 4], normal: [f32; 3]) -> Result<[f32; 3]> {
    let functions: Vec<_> = module.functions.iter()
        .filter(|(_, f)| f.name.as_deref().is_some_and(|name| name.ends_with("skin_normals"))).collect();
    if functions.len() != 1 { return Err("one actual skin_normals function required".into()); }
    let result = vector(call(module, functions[0].0, vec![Value::M(matrix.map(Vec::from).into()), Value::V(normal.into())])?)?;
    result.try_into().map_err(|_| "normal dimensions".into())
}
