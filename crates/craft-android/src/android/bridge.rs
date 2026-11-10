//! JNI: calls into `CraftActivity.bridge` and the native methods the activity calls.
//!
//! The JavaVM is android-activity's (`JavaVM::singleton`, same jni-rs release); the activity
//! registers itself with `nativeRegister` when it is created. Everything else crosses as JSON text
//! and byte arrays (crate::protocol).

use std::sync::{PoisonError, RwLock};

use jni::objects::{Global, JByteArray, JObject, JString};
use jni::{EnvUnowned, JValue, JavaVM, jni_sig, jni_str};

/// The current `CraftActivity` (replaced when Android recreates it).
static ACTIVITY: RwLock<Option<Global<JObject<'static>>>> = RwLock::new(None);

/// Whether an activity has registered.
pub fn available() -> bool {
    ACTIVITY.read().unwrap_or_else(PoisonError::into_inner).is_some()
}

/// `CraftActivity.bridge(op, json, data)`: its answer, or `None` (no answer, no activity, or a
/// failure, which is logged).
pub fn call(op: &str, json: &str, data: Option<&[u8]>) -> Option<Vec<u8>> {
    let guard = ACTIVITY.read().unwrap_or_else(PoisonError::into_inner);
    let activity = guard.as_ref()?;
    let vm = match JavaVM::singleton() {
        Ok(vm) => vm,
        Err(e) => {
            log::warn!("craft-android: no Java VM for {op}: {e}");
            return None;
        }
    };
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<Option<Vec<u8>>> {
        let op_j = env.new_string(op)?;
        let json_j = env.new_string(json)?;
        let data_j: JObject = match data {
            Some(d) => env.byte_array_from_slice(d)?.into(),
            None => JObject::null(),
        };
        let out = env
            .call_method(
                activity.as_obj(),
                jni_str!("bridge"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;[B)[B"),
                &[JValue::Object(&op_j), JValue::Object(&json_j), JValue::Object(&data_j)],
            )?
            .l()?;
        if out.is_null() {
            return Ok(None);
        }
        let bytes = env.cast_local::<JByteArray>(out)?;
        Ok(Some(env.convert_byte_array(&bytes)?))
    });
    result.unwrap_or_else(|e| {
        log::warn!("craft-android: bridge {op} failed: {e}");
        None
    })
}

/// `CraftActivity.nativeRegister(activity)`.
#[allow(unsafe_code)] // `no_mangle`: the JVM finds native methods by their exported name
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_craftandroid_CraftActivity_nativeRegister<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    activity: JObject<'caller>,
) {
    unowned
        .with_env(|env| -> jni::errors::Result<()> {
            let global = env.new_global_ref(&activity)?;
            *ACTIVITY.write().unwrap_or_else(PoisonError::into_inner) = Some(global);
            super::pickers::cancel_all();
            log::info!("craft-android: activity registered");
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// `CraftActivity.nativeEvent(op, json)`: a picked file, a document opened with the app.
#[allow(unsafe_code)] // `no_mangle`: the JVM finds native methods by their exported name
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_craftandroid_CraftActivity_nativeEvent<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    op: JString<'caller>,
    json: JString<'caller>,
) {
    unowned
        .with_env(|env| -> jni::errors::Result<()> {
            let op = op.try_to_string(env)?;
            let json = json.try_to_string(env)?;
            super::events::dispatch(&op, &json);
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// `CraftActivity.nativePointer(...)`: one mouse event (see crate::pointer).
#[allow(unsafe_code)] // `no_mangle`: the JVM finds native methods by their exported name
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)] // the JNI signature
pub extern "system" fn Java_io_github_craftandroid_CraftActivity_nativePointer<'caller>(
    _unowned: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    gone: jni::sys::jboolean,
    x: jni::sys::jfloat,
    y: jni::sys::jfloat,
    buttons: jni::sys::jint,
    hscroll: jni::sys::jfloat,
    vscroll: jni::sys::jfloat,
    meta: jni::sys::jint,
) {
    crate::pointer::push(crate::pointer::Pointer {
        gone,
        x,
        y,
        buttons: u32::try_from(buttons).unwrap_or(0) & 0x1f,
        wheel: (hscroll, vscroll),
        meta: u32::try_from(meta).unwrap_or(0),
    });
    super::events::wake();
}

/// `CraftActivity.nativeKey(...)`: one key of a hardware keyboard (see crate::keyboard).
#[allow(unsafe_code)] // `no_mangle`: the JVM finds native methods by their exported name
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)] // the JNI signature
pub extern "system" fn Java_io_github_craftandroid_CraftActivity_nativeKey<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    keycode: jni::sys::jint,
    base: jni::sys::jint,
    text: JString<'caller>,
    pressed: jni::sys::jboolean,
    repeat: jni::sys::jboolean,
    meta: jni::sys::jint,
) {
    unowned
        .with_env(|env| -> jni::errors::Result<()> {
            let text = if text.is_null() { String::new() } else { text.try_to_string(env)? };
            crate::keyboard::push(crate::keyboard::KeyPress {
                keycode,
                base: u32::try_from(base).unwrap_or(0),
                text,
                pressed,
                repeat,
                meta: u32::try_from(meta).unwrap_or(0),
            });
            super::events::wake();
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// `CraftActivity.nativeIme(text, start, end, reset)`: the on-screen keyboard's buffer (see
/// crate::ime).
#[allow(unsafe_code)] // `no_mangle`: the JVM finds native methods by their exported name
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_craftandroid_CraftActivity_nativeIme<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    text: JString<'caller>,
    start: jni::sys::jint,
    end: jni::sys::jint,
    reset: jni::sys::jboolean,
) {
    unowned
        .with_env(|env| -> jni::errors::Result<()> {
            let text = if text.is_null() { String::new() } else { text.try_to_string(env)? };
            crate::ime::push(&text, start, end, reset);
            super::events::wake();
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

/// `CraftActivity.nativeInsets(left, top, right, bottom)`: what the system covers, in pixels.
#[allow(unsafe_code)] // `no_mangle`: the JVM finds native methods by their exported name
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_craftandroid_CraftActivity_nativeInsets<'caller>(
    _unowned: EnvUnowned<'caller>,
    _this: JObject<'caller>,
    left: jni::sys::jint,
    top: jni::sys::jint,
    right: jni::sys::jint,
    bottom: jni::sys::jint,
) {
    #[allow(clippy::cast_precision_loss)] // window insets are small
    crate::insets::set(left as f32, top as f32, right as f32, bottom as f32);
    super::events::wake();
}
