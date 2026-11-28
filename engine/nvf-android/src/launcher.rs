//! Thin JNI boundary; this activity never calls the Bevy game entry point.
use jni::{objects::{JClass, JString}, sys::jstring, JNIEnv};

#[no_mangle]
pub extern "system" fn Java_org_nvforever_android_MobileLauncher_request(
    mut env: JNIEnv<'_>,
    class: JClass<'_>,
    home: JString<'_>,
    request: JString<'_>,
) -> jstring {
    let result = (|| -> jni::errors::Result<jstring> {
        let home: String = env.get_string(&home)?.into();
        let request: String = env.get_string(&request)?.into();
        let response = nvf_launcher::mobile::request(home.as_ref(), &request, |url| {
            env.with_local_frame(8, |env| -> jni::errors::Result<Option<String>> {
                let address = env.new_string(url)?;
                let text = env.call_static_method(&class, "getHttps", "(Ljava/lang/String;)Ljava/lang/String;", &[(&address).into()])?.l()?;
                if text.is_null() {
                    return Ok(None);
                }
                let text: JString<'_> = text.into();
                let body: String = env.get_string(&text)?.into();
                Ok(Some(body))
            }).map_err(|e| e.to_string())?.ok_or_else(|| format!("Could not read {url}"))
        });
        Ok(env.new_string(response)?.into_raw())
    })();
    match result {
        Ok(text) => text,
        Err(_) => {
            if !env.exception_check().unwrap_or(false) {
                let _ = env.throw_new("java/io/IOException", "Android launcher backend could not read its request");
            }
            std::ptr::null_mut()
        }
    }
}
