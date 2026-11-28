//! Bundled casing atlas with a local override. Live scene/text/actions belong to the
//! dialogue presentation, never to pixels captured in the supplied picture.
use bevy::{image::ImageSampler,prelude::*,render::render_resource::TextureFormat};
use nvf_assets::GameData;

pub(crate) const ASSET:&str="Textures/Interface/NVForever/Dialogue/classic.png";

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum Template {Wide, Legacy}

struct Atlas {
    size:UVec2,
    clear:[Rect;4],
    seeds:[UVec2;4],
    speech:Rect,
    responses:Rect,
    caps:Rect,
    red:Rect,
    trade:Rect,
}

const fn rect(x0:f32,y0:f32,x1:f32,y1:f32)->Rect {
    Rect {min:Vec2::new(x0,y0),max:Vec2::new(x1,y1)}
}

impl Template {
    pub(crate) fn size(self)->UVec2 {self.atlas().size}

    fn atlas(self)->&'static Atlas {
        match self {Self::Wide=>&WIDE,Self::Legacy=>&LEGACY}
    }
}

// Bounds and seeds identify the actual connected near-black openings, not a
// rectangular cutout. Separate inset rectangles keep text off rounded corners.
const WIDE:Atlas=Atlas {
    size:UVec2::new(1774,887),
    clear:[rect(42.,181.,554.,637.),rect(613.,181.,1737.,279.),
        rect(620.,326.,1734.,821.),rect(45.,737.,121.,782.)],
    seeds:[UVec2::new(283,390),UVec2::new(1188,230),UVec2::new(1170,549),UVec2::new(85,761)],
    speech:rect(630.,190.,1720.,268.), responses:rect(637.,341.,1717.,804.),
    caps:rect(50.,744.,115.,775.), red:rect(447.,754.,484.,792.),
    trade:rect(416.,713.,513.,801.),
};

// Keep the previously documented user-override format and its existing target.
const LEGACY:Atlas=Atlas {
    size:UVec2::new(1447,1087),
    clear:[rect(38.,231.,587.,813.),rect(631.,235.,1409.,393.),
        rect(633.,440.,1409.,1020.),rect(38.,920.,130.,982.)],
    seeds:[UVec2::new(300,500),UVec2::new(1000,310),UVec2::new(1000,700),UVec2::new(80,950)],
    speech:rect(665.,249.,1373.,376.), responses:rect(666.,466.,1376.,994.),
    caps:rect(42.,924.,126.,978.), red:rect(469.,950.,507.,982.),
    trade:rect(469.,950.,507.,982.),
};

/// Asset preparation only; Loaded does not assert a rendered frame or working input.
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub(crate) enum Status<'a> {WaitingForData,Invalid(&'a str),Loaded}

#[derive(Resource,Default)]
pub(crate) struct Cache {
    attempted:bool,
    error:Option<String>,
    art:Option<LoadedArt>,
}
impl Cache {
    /// Retry while GameData is absent; once it exists, read/decode at most once.
    /// A presentation/data reload can explicitly replace this cache with Default.
    pub(crate) fn prepare(&mut self,data:Option<&GameData>,images:&mut Assets<Image>)->Status<'_> {
        if !self.attempted {
            let Some(data)=data else{return Status::WaitingForData;};
            self.attempted=true;
            let image = match data.vfs.read(ASSET) {
                Some(Ok(bytes)) => decode(&bytes),
                Some(Err(error)) => Err(error.to_string()),
                None => decode_bundled(),
            };
            match image {
                Ok((image,template)) => self.art = Some(LoadedArt { image: images.add(image),template }),
                Err(error) => {
                    warn!("classic dialogue casing: {error}");
                    self.error = Some(error);
                }
            }
        }
        self.status()
    }
    pub(crate) fn status(&self)->Status<'_> {
        if self.art.is_some(){Status::Loaded}
        else if let Some(error)=self.error.as_deref(){Status::Invalid(error)}
        else {Status::WaitingForData}
    }
    pub(crate) fn loaded(&self)->Option<&LoadedArt> {self.art.as_ref()}
}

pub(crate) fn install(app:&mut App) {app.init_resource::<Cache>();}

#[cfg(not(target_os = "android"))]
fn decode_bundled() -> Result<(Image,Template), String> {
    decode(include_bytes!("../../assets/dialogue/classic.png"))
}

#[cfg(target_os = "android")]
fn decode_bundled() -> Result<(Image,Template), String> {
    let app = bevy::android::ANDROID_APP.get().ok_or("Android activity is unavailable")?;
    let mut asset = app.asset_manager().open(c"dialogue/classic.png")
        .ok_or("Bundled Classic dialogue casing is missing from the APK")?;
    decode(asset.buffer().map_err(|error| error.to_string())?)
}

fn decode(bytes:&[u8])->Result<(Image,Template),String> {
    let mut decoder=png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader=decoder.read_info().map_err(|e|e.to_string())?;
    // The source geometry only applies to these two atlas formats. Reject any
    // other dimensions before allocating the decoded pixel buffer.
    let size=UVec2::new(reader.info().width,reader.info().height);
    let template=[Template::Wide,Template::Legacy].into_iter().find(|t|t.size()==size)
        .ok_or_else(||format!("expected 1774x887 or 1447x1087 casing atlas, got {}x{}",size.x,size.y))?;
    let buffer_size=reader.output_buffer_size().ok_or("PNG output size is unavailable")?;
    let area=(size.x*size.y) as usize;
    if buffer_size>area*4 {
        return Err("casing PNG is not an 8-bit RGB/RGBA image".into());
    }
    let mut pixels=vec![0;buffer_size];
    let info=reader.next_frame(&mut pixels).map_err(|e|e.to_string())?;
    pixels.truncate(info.buffer_size());
    let mut rgba=match (info.color_type,info.bit_depth) {
        (png::ColorType::Rgba,png::BitDepth::Eight)=>pixels,
        (png::ColorType::Rgb,png::BitDepth::Eight)=>pixels.as_chunks::<3>().0.iter().flat_map(|p|[p[0],p[1],p[2],255]).collect(),
        _=>return Err("casing PNG is not an 8-bit RGB/RGBA image".into()),
    };
    let atlas=template.atlas();
    let mut seen=vec![false;area];
    let mut pending=Vec::new();
    for (bounds,seed) in atlas.clear.into_iter().zip(atlas.seeds) {
        if clear_opening(&mut rgba,size,bounds,seed,&mut seen,&mut pending)<16 {
            return Err("casing template has no expected clear opening".into());
        }
    }
    let mut image=nvf_assets::texture::checked_image(size.x,size.y,1,TextureFormat::Rgba8UnormSrgb,rgba)?;
    image.sampler=ImageSampler::linear();
    image.texture_descriptor.label=Some("supplied classic dialogue casing");
    Ok((image,template))
}

// Four-neighbour fill stays inside each permitted aperture and clears only its
// connected black/transparent pixels. Dark metal elsewhere is never selected.
fn clear_opening(rgba:&mut [u8],size:UVec2,bounds:Rect,seed:UVec2,seen:&mut [bool],pending:&mut Vec<UVec2>)->usize {
    pending.push(seed);
    let mut count=0;
    while let Some(p)=pending.pop() {
        if p.x>=size.x || p.y>=size.y || !bounds.contains(p.as_vec2()+Vec2::splat(0.5)) {continue;}
        let index=(p.y*size.x+p.x) as usize;
        if std::mem::replace(&mut seen[index],true) {continue;}
        let pixel=&mut rgba[index*4..index*4+4];
        if pixel[3]!=0 && pixel[..3].iter().any(|v|*v>8) {continue;}
        pixel.fill(0);count+=1;
        if p.x>0 {pending.push(UVec2::new(p.x-1,p.y));}
        if p.y>0 {pending.push(UVec2::new(p.x,p.y-1));}
        if p.x+1<size.x {pending.push(UVec2::new(p.x+1,p.y));}
        if p.y+1<size.y {pending.push(UVec2::new(p.x,p.y+1));}
    }
    count
}

/// One atlas crop and where it is drawn, in safe-canvas logical units.
#[derive(Clone,Copy,Debug,PartialEq)]
pub(crate) struct Part {pub(crate) source:Rect,pub(crate) destination:Rect}

/// Caps text metrics per unit of glyph size, for the exact value drawn.
#[derive(Clone,Copy,Debug,PartialEq)]
pub(crate) struct CapsText {pub(crate) left:f32,pub(crate) width:f32,pub(crate) height:f32}
impl CapsText {
    /// Logical start of the first glyph inside the counter opening.
    pub(crate) fn left(&self,size:f32)->f32 {self.left*size+1.}
}

/// [design: docs/design/android.md] Full-viewport Classic dialogue layout: the
/// entire `dialogue/classic.png` image stretches cleanly to 100% × 100% of the
/// window (zero black borders, no edge-only strip stretching) and every opening
/// and interactive region maps proportionally inside that stretched image.
#[derive(Clone,Copy,Debug,PartialEq)]
pub(crate) struct Layout {
    pub(crate) template:Template,
    pub(crate) size:Vec2,
    pub(crate) full:Rect,
    pub(crate) world:Rect,
    pub(crate) speech:Rect,
    pub(crate) responses:Rect,
    pub(crate) caps:Rect,
    pub(crate) caps_size:f32,
    pub(crate) wells:[Rect;3],
    pub(crate) red_button:Rect,
    pub(crate) trade:Rect,
}
impl Layout {
    /// `bleed` is the left, top, right and bottom safe-area inset around `size`.
    pub(crate) fn new(template:Template,size:Vec2,bleed:[f32;4],caps:CapsText)->Option<Self> {
        if !size.is_finite() || size.min_element()<=0. || bleed.iter().any(|b|!b.is_finite() || *b<0.)
            || !caps.width.is_finite() || caps.width<=0. || !caps.height.is_finite() || caps.height<=0. {return None;}
        let atlas=template.atlas();
        let source=atlas.size.as_vec2();
        // Stretch the entire supplied image across 100% x 100% of the window.
        let full=Rect::new(-bleed[0],-bleed[1],size.x+bleed[2],size.y+bleed[3]);
        let full_size=full.size();
        if !full_size.is_finite() || full_size.min_element()<=0. {return None;}
        let scale=full_size/source;
        let map=|r:Rect| Rect::new(
            full.min.x+r.min.x*scale.x,
            full.min.y+r.min.y*scale.y,
            full.min.x+r.max.x*scale.x,
            full.min.y+r.max.y*scale.y,
        );
        let safe=Rect::from_corners(Vec2::ZERO,size);
        let [portrait,speech_well,responses_well,counter_well]=atlas.clear;
        let world=map(portrait);
        let wells=[map(speech_well),map(responses_well),map(counter_well)];
        let speech=map(atlas.speech).intersect(safe);
        let responses=map(atlas.responses).intersect(safe);
        let caps_rect=map(atlas.caps);
        let red=map(atlas.red);
        let plate=map(atlas.trade);
        let minimum=Rect::from_center_size(red.center(),Vec2::splat(48.));
        let trade=Rect::from_corners(plate.min.min(minimum.min),plate.max.max(minimum.max)).intersect(full);
        // Scale caps glyphs to fit both the counter height and width cleanly without truncation.
        let max_by_height=caps_rect.height()/caps.height;
        let max_by_width=((caps_rect.width()-4.0).max(1.0))/caps.width;
        let caps_size=max_by_height.min(max_by_width);
        if !caps_size.is_finite() || caps_size<=0.0 {return None;}
        let inside_full=|r:Rect| r.min.cmpge(full.min-0.01).all() && r.max.cmple(full.max+0.01).all();
        if [world,speech,responses,caps_rect,trade].into_iter().any(|r| !inside_full(r) || r.width()<=0. || r.height()<=0.)
            || world.size().min_element()<48. || speech.width()<48. || speech.height()<24.
            || responses.width()<140. || responses.height()<96. || trade.size().min_element()<47.99
            || trade.intersect(caps_rect).size().min_element()>0.
            || [world,speech,responses].into_iter().any(|r| trade.intersect(r).size().min_element()>0.) {return None;}
        Some(Self {template,size,full,world,speech,responses,caps:caps_rect,caps_size,wells,red_button:red,trade})
    }

    /// Single full-atlas stretched casing covering 100% x 100% of the window.
    pub(crate) fn parts(&self)->impl Iterator<Item=Part>+'_ {
        let source=Rect::from_corners(Vec2::ZERO,self.template.size().as_vec2());
        std::iter::once(Part {source,destination:self.full})
    }
}

pub(crate) struct LoadedArt {image:Handle<Image>,pub(crate) template:Template}
impl LoadedArt {
    pub(crate) fn image_id(&self)->bevy::asset::AssetId<Image> {self.image.id()}
    /// Caller places the parent at the safe-canvas origin; bleed crops extend past
    /// it to the window edges. Divide by Bevy UiScale exactly once; DPI already
    /// belongs to the camera's normal UI provider. The real housing presenter uses
    /// opacity 1.0; legacy frame opacity belongs only to Field Notes.
    pub(crate) fn spawn(&self,parent:&mut ChildSpawnerCommands,layout:&Layout,ui_scale:f32,opacity:f32)->bool {
        if !ui_scale.is_finite() || ui_scale<=0. || !opacity.is_finite() {return false;}
        if layout.parts().any(|p|!(p.destination.min/ui_scale).is_finite()
            || !(p.destination.max/ui_scale).is_finite()) {return false;}
        for part in layout.parts() {
            let r=part.destination;
            parent.spawn((
                Node {position_type:PositionType::Absolute,left:px(r.min.x/ui_scale),top:px(r.min.y/ui_scale),
                    width:px(r.width()/ui_scale),height:px(r.height()/ui_scale),..default()},
                ImageNode {image:self.image.clone(),rect:Some(part.source),color:Color::WHITE.with_alpha(opacity.clamp(0.,1.)),
                    image_mode:NodeImageMode::Stretch,..default()},
                Pickable::IGNORE,
            ));
        }
        true
    }
}


#[cfg(test)]
pub(super) mod tests {
    use super::*;
    fn authored_png()->Vec<u8> {
        let UVec2 {x:width,y:height}=LEGACY.size;
        let mut bytes=Vec::new();
        {
            let mut encoder=png::Encoder::new(&mut bytes,width,height);
            encoder.set_color(png::ColorType::Rgba);encoder.set_depth(png::BitDepth::Eight);
            let mut writer=encoder.write_header().unwrap();
            let pixels:Vec<_>=(0..width*height).flat_map(|index| {
                let p=Vec2::new((index%width) as f32+0.5,(index/width) as f32+0.5);
                if LEGACY.clear.iter().any(|r|r.contains(p)) {[0,0,0,255]}else{[37,51,73,255]}
            }).collect();
            writer.write_image_data(&pixels).unwrap();
        }
        bytes
    }
    pub(crate) struct Files(pub std::path::PathBuf);
    impl Drop for Files {fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
    pub(crate) fn files(with_font:bool)->(Files,GameData) {
        files_with_art(with_font, true)
    }
    pub(crate) fn files_with_art(with_font: bool, override_art: bool) -> (Files, GameData) {
        let root=std::env::temp_dir().join(format!("nvf-classic-presenter-{}-{}",std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let files = Files(root);
        std::fs::create_dir_all(&files.0).unwrap();
        if override_art {
            let path = files.0.join(ASSET);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, authored_png()).unwrap();
        }
        if with_font {
            // Independently authored bitmap font, through the real .fnt/.tex reader.
            let name=crate::interface::font_file(crate::interface::FONT_FILES[4]).replace('\\',"/");
            assert!(!std::path::Path::new(&name).is_absolute() && !name.split('/').any(|p|p==".."));
            let path=files.0.join(name);std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut bytes=vec![0;fnv_font::FILE_SIZE];
            bytes[..4].copy_from_slice(&25.0_f32.to_le_bytes());bytes[4..8].copy_from_slice(&1_u32.to_le_bytes());
            bytes[12..26].copy_from_slice(b"ClassicFixture");
            for code in 0..256 {
                let mut put=|slot:usize,v:f32| {
                    let at=296+code*56+slot*4;bytes[at..at+4].copy_from_slice(&v.to_le_bytes());
                };
                if code==32 {put(12,8.0);}else if (33..127).contains(&code) {
                    for (slot,value) in [(1,0.1),(2,0.1),(7,0.9),(8,0.9),(9,12.0),(10,20.0),(12,1.0),(13,20.0)] {put(slot,value);}
                }
            }
            std::fs::write(path,bytes).unwrap();
            let texture=files.0.join("textures/fonts/classicfixture.tex");std::fs::create_dir_all(texture.parent().unwrap()).unwrap();
            // Use an atlas tile large enough to model the softened bezel glyphs at 12 units.
            let mut pixels=32_u32.to_le_bytes().to_vec();pixels.extend(32_u32.to_le_bytes());pixels.extend([255;32*32*4]);
            std::fs::write(&texture,pixels).unwrap();
            // Main admits TEX only when its companion DDS also exists.
            std::fs::write(texture.with_extension("dds"), []).unwrap();
        }
        let data=GameData::new(std::sync::Arc::new(fnv_vfs::Vfs::open(&files.0,&[]).unwrap()),
            std::sync::Arc::new(fnv_nif::Schema::load_dir(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"),"/../../schemas/nif"))).unwrap()));
        (files,data)
    }
    #[test]
    fn supplied_image_decoder_removes_captured_content_and_keeps_hardware_pixels() {
        let (image,template)=decode(&authored_png()).unwrap();
        assert_eq!(template,Template::Legacy);
        assert_eq!(image.size(),LEGACY.size);
        let bytes=image.data.as_ref().unwrap();
        for seed in LEGACY.seeds {let at=((seed.y*LEGACY.size.x+seed.x)*4) as usize;assert_eq!(&bytes[at..at+4],&[0,0,0,0]);}
        for (x,y) in [(100,100),(1200,100),(600,900),(400,1000)] {
            let at=((y*LEGACY.size.x+x)*4) as usize;assert_eq!(&bytes[at..at+4],&[37,51,73,255]);
        }
        assert!(decode(b"not a PNG").is_err());
        let mut short=Vec::new();{
            let mut encoder=png::Encoder::new(&mut short,1,1);encoder.set_color(png::ColorType::Rgba);
            encoder.write_header().unwrap().write_image_data(&[0;4]).unwrap();
        }
        assert!(decode(&short).is_err(),"a different atlas cannot use these source apertures");
    }
    #[test]
    fn bundled_wide_atlas_clears_the_four_black_openings_and_keeps_rounded_hardware() {
        let (image,template)=decode(include_bytes!("../../assets/dialogue/classic.png")).unwrap();
        assert_eq!(template,Template::Wide);
        assert_eq!(image.size(),UVec2::new(1774,887));
        let bytes=image.data.as_ref().unwrap();
        let pixel=|x:u32,y:u32| {let start=((y*1774+x)*4) as usize;&bytes[start..start+4]};
        for (x,y) in [(42,395),(553,395),(297,181),(297,636)] {
            assert_eq!(pixel(x,y),[0;4],"the portrait opens to the live camera at {x},{y}");
        }
        // All source pixels in the text rectangles are clear, not just their seeds.
        for text in [WIDE.speech,WIDE.responses,WIDE.caps] {
            for y in text.min.y as u32..text.max.y as u32 {
                for x in text.min.x as u32..text.max.x as u32 {
                    assert_eq!(pixel(x,y),[0;4],"text overlaps casing at {x},{y}");
                }
            }
        }
        for ((x,y),rgba) in [((43,182),[59,40,14,255]),((614,182),[27,21,16,255]),
            ((621,327),[65,56,46,255]),((45,737),[11,9,3,255]),((465,773),[214,2,1,255])] {
            assert_eq!(pixel(x,y),rgba,"rounded hardware/Trade remains at {x},{y}");
        }
    }
    #[test]
    fn runtime_vfs_override_is_loaded_once_and_malformed_data_is_refused() {
        let (files,data)=files(false);let mut cache=Cache::default();let mut images=Assets::<Image>::default();
        assert_eq!(cache.prepare(None,&mut images),Status::WaitingForData);assert!(cache.loaded().is_none());
        assert_eq!(cache.prepare(Some(&data),&mut images),Status::Loaded);
        let image=cache.loaded().unwrap().image.id();assert_eq!(images.len(),1);
        std::fs::remove_file(files.0.join(ASSET)).unwrap();
        assert_eq!(cache.prepare(Some(&data),&mut images),Status::Loaded);
        assert_eq!(cache.loaded().unwrap().image.id(),image);assert_eq!(images.len(),1,"no per-frame decode");
        std::fs::write(files.0.join(ASSET), b"not a PNG").unwrap();
        let mut malformed = Cache::default();
        assert!(matches!(malformed.prepare(Some(&data), &mut images), Status::Invalid(_)));
        assert!(malformed.loaded().is_none(), "an invalid override is not silently replaced");
    }
    #[derive(Resource)]
    struct Fixture {art:LoadedArt,layout:Layout,scale:f32}
    fn paint(mut commands:Commands,fixture:Res<Fixture>) {
        commands.spawn(Node::default()).with_children(|p|assert!(fixture.art.spawn(p,&fixture.layout,fixture.scale,1.0)));
    }
    /// Font5-like metrics: a 0.52-em advance per digit and a 1.2-em cell.
    fn caps(chars:usize)->CapsText {CapsText {left:0.0,width:0.52*chars as f32,height:1.2}}

    #[test]
    fn reflow_preserves_content_openings_and_spawns_only_passive_atlas_parts() {
        let mut images=Assets::<Image>::default();
        let (image,template)=decode(include_bytes!("../../assets/dialogue/classic.png")).unwrap();
        let handle=images.add(image);
        for width in [640.,800.] {for scale in [0.75,1.,1.5] {
            let layout=Layout::new(Template::Wide,Vec2::new(width,360.),[0.;4],caps(1)).unwrap();
            assert!(layout.world.height()>=48. && layout.responses.height()>=48.);
            assert!(layout.trade.size().min_element()>=47.99);
            assert!(Layout::new(Template::Wide,Vec2::new(width,120.),[0.;4],caps(1)).is_none(),"no hidden target shrinking");
            let mut app=App::new();
            let art=LoadedArt {image:handle.clone(),template};
            app.insert_resource(Fixture {art,layout,scale}).add_systems(Update,paint);app.update();
            let mut pieces=app.world_mut().query::<(&Node,&ImageNode,&Pickable)>();let mut count=0;
            for (node,image,picking) in pieces.iter(app.world()) {
                count+=1;assert_eq!(*picking,Pickable::IGNORE);assert_eq!(image.color,Color::WHITE);
                let (Val::Px(x),Val::Px(y),Val::Px(w),Val::Px(h))=(node.left,node.top,node.width,node.height) else{panic!("actual positioned atlas piece");};
                assert!(x>=-0.01 && y>=-0.01 && w>0. && h>0.);
                assert!((x+w)*scale<=width+0.01 && (y+h)*scale<=360.01);
                let uv=image.rect.unwrap();
                assert_eq!(uv,Rect::from_corners(Vec2::ZERO,template.size().as_vec2()));
            }
            assert_eq!(count,1,"single full-screen stretched classic casing image");
        }}
    }

    #[test]
    fn classic_stretches_entire_image_to_fill_every_aspect_with_zero_black_borders() {
        // Logical windows: 20:9, 19.5:9, 16:9, 4:3, a phone and a tablet in portrait,
        // each with a cutout/status/navigation inset the stretched image must cover 100% x 100%.
        let cases=[
            (Vec2::new(880.,396.),[34.,0.,34.,16.]),(Vec2::new(845.,390.),[0.,0.,32.,20.]),
            (Vec2::new(800.,450.),[0.;4]),(Vec2::new(1024.,768.),[0.,0.,0.,20.]),
            (Vec2::new(393.,873.),[0.,28.,0.,24.]),(Vec2::new(800.,1280.),[0.,24.,0.,0.]),
        ];
        for (window,[l,t,r,b]) in cases {
            let safe=window-Vec2::new(l+r,t+b);
            let layout=Layout::new(Template::Wide,safe,[l,t,r,b],caps(5)).unwrap_or_else(||panic!("Classic admitted at {window}"));
            let parts:Vec<_>=layout.parts().collect();
            assert_eq!(parts.len(),1,"entire classic.png stretches as a single full-screen image");
            let full=parts[0];
            assert_eq!(full.source,Rect::from_corners(Vec2::ZERO,WIDE.size.as_vec2()));
            assert_eq!(full.destination,Rect::new(-l,-t,safe.x+r,safe.y+b));
            assert!((full.destination.width()-window.x).abs()<0.01 && (full.destination.height()-window.y).abs()<0.01);
            for (text,well) in [(layout.speech,layout.wells[0]),(layout.responses,layout.wells[1]),(layout.caps,layout.wells[2])] {
                assert!(well.inflate(0.01).contains(text.min) && well.inflate(0.01).contains(text.max),"{text:?} outside {well:?} at {window}");
            }
            assert!(layout.speech.height()>=24. && layout.responses.height()>=96. && layout.responses.width()>=140.);
            assert!(layout.trade.size().min_element()>=47.99);
        }
    }
}
