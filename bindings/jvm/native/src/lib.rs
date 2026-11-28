//! The JNI library of kakemono's JVM binding: the native methods of
//! `io.github.iamcaominhtien.kakemono.Native`. The Java classes own the
//! handles (a `long`, a boxed [`Document`]) and never use one after freeing
//! it nor free it while a call runs (`Document` keeps a read-write lock).
//!
//! Text comes back as UTF-8 `byte[]`, decoded in Java: faster than JNI's
//! modified UTF-8 for pages of several megabytes. An error is thrown as a
//! `KakemonoException` with its kind; no panic crosses into the JVM.
#![allow(unsafe_code)]

use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JObject, JObjectArray, JString, JValue};
use jni::sys::{jdouble, jint, jlong, jobject, jobjectArray, jstring};
use kakemono_bindings::{CancelToken, Cancelled, Document, Error, ErrorKind, OpenOptions, RenderOptions, Source};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// The exception the Java side throws.
const EXCEPTION: &str = "io/github/iamcaominhtien/kakemono/KakemonoException";

/// The `kind` of an error, as `KakemonoException.Kind` numbers it.
fn kind_code(kind: ErrorKind) -> jint {
    match kind {
        ErrorKind::InvalidArgument => 1,
        ErrorKind::Document => 2,
        ErrorKind::Internal => 3,
        ErrorKind::Cancelled => 4,
        ErrorKind::TimedOut => 5,
    }
}

/// An error of JNI itself (a failed allocation, a pending exception).
fn jni_error(err: jni::errors::Error) -> Error {
    Error::new(ErrorKind::Internal, format!("JNI: {err}"))
}

/// Throws `err` as a `KakemonoException`, unless an exception is already
/// pending.
fn throw(env: &mut JNIEnv<'_>, err: &Error) {
    if env.exception_check().unwrap_or(true) {
        return;
    }
    let thrown = (|| -> jni::errors::Result<()> {
        let message = env.new_string(err.message())?;
        let exception = env.new_object(
            EXCEPTION,
            "(Ljava/lang/String;I)V",
            &[JValue::Object(&message), JValue::Int(kind_code(err.kind()))],
        )?;
        env.throw(jni::objects::JThrowable::from(exception))
    })();
    if thrown.is_err() && !env.exception_check().unwrap_or(true) {
        let _ = env.throw_new("java/lang/RuntimeException", err.message());
    }
}

/// Runs `work`, throwing its error (or a panic) into the JVM and returning
/// `fallback` then.
fn run<'local, T>(
    env: &mut JNIEnv<'local>,
    fallback: T,
    work: impl FnOnce(&mut JNIEnv<'local>) -> Result<T, Error>,
) -> T {
    let result = catch_unwind(AssertUnwindSafe(|| work(env))).unwrap_or_else(|panic| {
        let what = kakemono_bindings::panic_message(&*panic);
        Err(Error::new(
            ErrorKind::Internal,
            format!("internal error in kakemono: {what}"),
        ))
    });
    match result {
        Ok(value) => value,
        Err(err) => {
            throw(env, &err);
            fallback
        }
    }
}

/// A Java string (`null`: `None`).
fn read_string(env: &mut JNIEnv<'_>, text: &JString<'_>) -> Result<Option<String>, Error> {
    if text.is_null() {
        return Ok(None);
    }
    Ok(Some(env.get_string(text).map_err(jni_error)?.into()))
}

/// The document of a handle.
fn document<'a>(handle: jlong) -> Result<&'a Document, Error> {
    if handle == 0 {
        return Err(Error::invalid("the document is closed"));
    }
    // SAFETY: the Java side gives a handle of `open` not yet freed, and does
    // not free it while this call runs.
    Ok(unsafe { &*(handle as *const Document) })
}

/// The token a call runs with: that of the handle `cancel` (0 for none),
/// with the time limit `timeout_ms` (0 for none).
fn stop(cancel: jlong, timeout_ms: jdouble) -> Result<Option<CancelToken>, Error> {
    // SAFETY: the Java side gives a handle of `cancelNew` not yet freed, and
    // keeps its `CancelToken` reachable while this call runs.
    let parent = (cancel != 0).then(|| unsafe { &*(cancel as *const CancelToken) });
    let limit = if timeout_ms == 0.0 {
        None
    } else {
        kakemono_bindings::timeout_millis(Some(timeout_ms))?
    };
    Ok(kakemono_bindings::stop_token(parent, limit))
}

/// A time limit in milliseconds for a new token: 0 for none.
fn limit(timeout_ms: jdouble) -> Result<Option<std::time::Duration>, Error> {
    if timeout_ms == 0.0 {
        Ok(None)
    } else {
        kakemono_bindings::timeout_millis(Some(timeout_ms))
    }
}

/// Opens `source` with the JSON options `options`, as a handle.
fn open(
    env: &mut JNIEnv<'_>,
    source: Source,
    options: &JString<'_>,
    cancel: jlong,
    timeout_ms: jdouble,
) -> Result<jlong, Error> {
    let opts = OpenOptions::from_json(read_string(env, options)?.as_deref())?;
    let doc = Document::open_with(source, &opts, stop(cancel, timeout_ms)?.as_ref())?;
    Ok(Box::into_raw(Box::new(doc)) as jlong)
}

/// A new Java `byte[]` of `data`.
fn bytes<'local>(env: &mut JNIEnv<'local>, data: &[u8]) -> Result<JObject<'local>, Error> {
    Ok(env.byte_array_from_slice(data).map_err(jni_error)?.into())
}

/// A new Java `String[]` of `items`.
fn strings<'local>(env: &mut JNIEnv<'local>, items: &[String]) -> Result<JObjectArray<'local>, Error> {
    let array = env
        .new_object_array(len(items.len())?, "java/lang/String", JObject::null())
        .map_err(jni_error)?;
    for (at, item) in items.iter().enumerate() {
        let text = env.new_string(item).map_err(jni_error)?;
        env.set_object_array_element(&array, len(at)?, &text)
            .map_err(jni_error)?;
        env.delete_local_ref(text).map_err(jni_error)?;
    }
    Ok(array)
}

/// A new Java `int[]` of `items`.
fn ints<'local>(env: &mut JNIEnv<'local>, items: &[usize]) -> Result<JObject<'local>, Error> {
    let values = items.iter().map(|&item| len(item)).collect::<Result<Vec<_>, _>>()?;
    let array = env.new_int_array(len(values.len())?).map_err(jni_error)?;
    env.set_int_array_region(&array, 0, &values).map_err(jni_error)?;
    Ok(array.into())
}

/// A size as a Java `int`.
fn len(value: usize) -> Result<jint, Error> {
    jint::try_from(value).map_err(|_| Error::new(ErrorKind::Internal, "too large for the JVM"))
}

/// A new Java `Integer`.
fn integer<'local>(env: &mut JNIEnv<'local>, value: usize) -> Result<JObject<'local>, Error> {
    let value = len(value)?;
    env.call_static_method(
        "java/lang/Integer",
        "valueOf",
        "(I)Ljava/lang/Integer;",
        &[JValue::Int(value)],
    )
    .and_then(jni::objects::JValueGen::l)
    .map_err(jni_error)
}

/// `int abiVersion()`: the version of these native methods, checked by
/// `Native` when it loads the library.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_abiVersion(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
) -> jint {
    2
}

/// `String version()`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_version<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    run(&mut env, std::ptr::null_mut(), |env| {
        Ok(env
            .new_string(kakemono_bindings::version())
            .map_err(jni_error)?
            .into_raw())
    })
}

/// `int addFont(byte[] data)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_addFont<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data: JByteArray<'local>,
) -> jint {
    run(&mut env, 0, |env| {
        let data = env.convert_byte_array(&data).map_err(jni_error)?;
        len(kakemono_bindings::add_font(data)?)
    })
}

/// `long openData(byte[] data, String options, long cancel, double timeoutMs)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_openData<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    data: JByteArray<'local>,
    options: JString<'local>,
    cancel: jlong,
    timeout_ms: jdouble,
) -> jlong {
    run(&mut env, 0, |env| {
        let data = env.convert_byte_array(&data).map_err(jni_error)?;
        open(env, Source::Data(data), &options, cancel, timeout_ms)
    })
}

/// Defines `long <name>(String text, String options, long cancel, double
/// timeoutMs)`, opening the source
/// `variant` of the text.
macro_rules! open_from_text {
    ($name:ident, $variant:ident, $what:literal) => {
        #[doc = concat!("`long ", $what, "(String, String options, long cancel, double timeoutMs)`.")]
        #[unsafe(no_mangle)]
        pub extern "system" fn $name<'local>(
            mut env: JNIEnv<'local>,
            _class: JClass<'local>,
            text: JString<'local>,
            options: JString<'local>,
            cancel: jlong,
            timeout_ms: jdouble,
        ) -> jlong {
            run(&mut env, 0, |env| {
                let text = read_string(env, &text)?.ok_or_else(|| Error::invalid(concat!($what, ": null")))?;
                open(env, Source::$variant(text), &options, cancel, timeout_ms)
            })
        }
    };
}

open_from_text!(Java_io_github_iamcaominhtien_kakemono_Native_openPath, Path, "openPath");
open_from_text!(Java_io_github_iamcaominhtien_kakemono_Native_openUrl, Url, "openUrl");
open_from_text!(
    Java_io_github_iamcaominhtien_kakemono_Native_openArxiv,
    Arxiv,
    "openArxiv"
);

/// `void free(long handle)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_free(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    handle: jlong,
) {
    if handle != 0 {
        // SAFETY: the Java side frees a handle of `open` once, with no call
        // running on it.
        let _ = catch_unwind(|| drop(unsafe { Box::from_raw(handle as *mut Document) }));
    }
}

/// `String format(long handle)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_format<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jstring {
    run(&mut env, std::ptr::null_mut(), |env| {
        let doc = document(handle)?;
        Ok(env.new_string(doc.format()).map_err(jni_error)?.into_raw())
    })
}

/// `int pageCount(long handle)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_pageCount<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jint {
    run(&mut env, 0, |_| len(document(handle)?.page_count()))
}

/// `byte[] title(long handle)`: UTF-8, or `null`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_title<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jobject {
    run(&mut env, std::ptr::null_mut(), |env| match document(handle)?.title() {
        Some(title) => Ok(bytes(env, title.as_bytes())?.into_raw()),
        None => Ok(std::ptr::null_mut()),
    })
}

/// `String[] warnings(long handle)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_warnings<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jobjectArray {
    run(&mut env, std::ptr::null_mut(), |env| {
        let warnings = document(handle)?.warnings().to_vec();
        Ok(strings(env, &warnings)?.into_raw())
    })
}

/// `Object[] render(long handle, String target, String options)`:
/// `{ byte[] text (UTF-8) or null, byte[] data or null, String[] paths,
/// byte[][] files }`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_render<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    target: JString<'local>,
    options: JString<'local>,
    cancel: jlong,
    timeout_ms: jdouble,
) -> jobjectArray {
    run(&mut env, std::ptr::null_mut(), |env| {
        let doc = document(handle)?;
        let target = read_string(env, &target)?.ok_or_else(|| Error::invalid("target: null"))?;
        let opts = read_string(env, &options)?;
        let out = doc.render_with(
            target.parse()?,
            &RenderOptions::from_json(opts.as_deref())?,
            stop(cancel, timeout_ms)?.as_ref(),
        )?;
        let result = env
            .new_object_array(4, "java/lang/Object", JObject::null())
            .map_err(jni_error)?;
        if let Some(text) = &out.text {
            let text = bytes(env, text.as_bytes())?;
            env.set_object_array_element(&result, 0, text).map_err(jni_error)?;
        }
        if let Some(data) = &out.data {
            let data = bytes(env, data)?;
            env.set_object_array_element(&result, 1, data).map_err(jni_error)?;
        }
        let paths: Vec<String> = out.files.iter().map(|file| file.path.clone()).collect();
        let paths = strings(env, &paths)?;
        env.set_object_array_element(&result, 2, paths).map_err(jni_error)?;
        let files = env
            .new_object_array(len(out.files.len())?, "[B", JObject::null())
            .map_err(jni_error)?;
        for (at, file) in out.files.iter().enumerate() {
            let data = bytes(env, &file.data)?;
            env.set_object_array_element(&files, len(at)?, &data)
                .map_err(jni_error)?;
            env.delete_local_ref(data).map_err(jni_error)?;
        }
        env.set_object_array_element(&result, 3, files).map_err(jni_error)?;
        Ok(result.into_raw())
    })
}

/// `Object[] chunks(long handle, String options)`: for each chunk,
/// `{ Integer id, byte[] text, Integer tokens, byte[] title or null,
/// String[] section, int[] blocks, int[] pages }`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_chunks<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    options: JString<'local>,
    cancel: jlong,
    timeout_ms: jdouble,
) -> jobjectArray {
    run(&mut env, std::ptr::null_mut(), |env| {
        let doc = document(handle)?;
        let opts = read_string(env, &options)?;
        let chunks = doc.chunks_with(
            &RenderOptions::from_json(opts.as_deref())?,
            stop(cancel, timeout_ms)?.as_ref(),
        )?;
        let result = env
            .new_object_array(len(chunks.len())?, "java/lang/Object", JObject::null())
            .map_err(jni_error)?;
        for (at, chunk) in chunks.iter().enumerate() {
            // The local references of a chunk are deleted once it is set: a
            // long book has thousands of chunks.
            let item = env
                .new_object_array(7, "java/lang/Object", JObject::null())
                .map_err(jni_error)?;
            let fields = [
                integer(env, chunk.id)?,
                bytes(env, chunk.text.as_bytes())?,
                integer(env, chunk.tokens)?,
                match &chunk.title {
                    Some(title) => bytes(env, title.as_bytes())?,
                    None => JObject::null(),
                },
                strings(env, &chunk.section)?.into(),
                ints(env, &chunk.blocks)?,
                ints(env, &chunk.pages)?,
            ];
            for (index, field) in fields.into_iter().enumerate() {
                env.set_object_array_element(&item, len(index)?, &field)
                    .map_err(jni_error)?;
                env.delete_local_ref(field).map_err(jni_error)?;
            }
            env.set_object_array_element(&result, len(at)?, &item)
                .map_err(jni_error)?;
            env.delete_local_ref(item).map_err(jni_error)?;
        }
        Ok(result.into_raw())
    })
}

/// `long cancelNew(long parent, double timeoutMs)`: a new token (a child of
/// the token `parent` when it is not 0), with the time limit `timeoutMs`
/// (0 for none).
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_cancelNew<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    parent: jlong,
    timeout_ms: jdouble,
) -> jlong {
    run(&mut env, 0, |_| {
        let limit = limit(timeout_ms)?;
        let token = if parent == 0 {
            limit.map_or_else(CancelToken::new, CancelToken::with_timeout)
        } else {
            // SAFETY: the Java side gives a live token handle.
            unsafe { &*(parent as *const CancelToken) }.child(limit)
        };
        Ok(Box::into_raw(Box::new(token)) as jlong)
    })
}

/// `void cancelCancel(long token)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_cancelCancel(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
) {
    if token != 0 {
        // SAFETY: the Java side gives a live token handle.
        let _ = catch_unwind(|| unsafe { &*(token as *const CancelToken) }.cancel());
    }
}

/// `int cancelState(long token)`: 0 while the work may go on, 4 once it is
/// cancelled, 5 once it is past its time limit (the kinds of
/// `KakemonoException`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_cancelState(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
) -> jint {
    if token == 0 {
        return 0;
    }
    // SAFETY: the Java side gives a live token handle.
    match catch_unwind(|| unsafe { &*(token as *const CancelToken) }.state()) {
        Ok(Some(Cancelled::TimedOut)) => 5,
        Ok(Some(_)) => 4,
        _ => 0,
    }
}

/// `void cancelFree(long token)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_iamcaominhtien_kakemono_Native_cancelFree(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
) {
    if token != 0 {
        // SAFETY: the Java side frees a token handle once, when no call holds it.
        let _ = catch_unwind(|| drop(unsafe { Box::from_raw(token as *mut CancelToken) }));
    }
}
