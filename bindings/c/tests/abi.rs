//! The C ABI, called as C calls it: ownership, errors, (pointer, length)
//! strings.
#![allow(unsafe_code)]

use kakemono_ffi::*;
use std::ffi::{CString, c_char};
use std::ptr;

/// The text at (`ptr`, `len`).
fn text(ptr: *const c_char, len: usize) -> String {
    assert!(!ptr.is_null());
    // SAFETY: the ABI gives `len` bytes followed by a NUL.
    let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len + 1) };
    assert_eq!(bytes[len], 0, "a NUL follows the text");
    String::from_utf8(bytes[..len].to_vec()).unwrap()
}

/// The path of a test file.
fn sample(path: &str) -> CString {
    CString::new(format!("{}/../../tests/data/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn a_document_renders_and_frees() {
    unsafe {
        assert_eq!(kk_abi_version(), KK_ABI_VERSION);
        let mut len = 0;
        assert_eq!(text(kk_version(&mut len), len), env!("CARGO_PKG_VERSION"));
        let mut err = ptr::null_mut();
        let opts = CString::new(r#"{"jobs":1}"#).unwrap();
        let doc = kk_open_path(sample("word/sample.docx").as_ptr(), opts.as_ptr(), &mut err);
        assert!(!doc.is_null() && err.is_null());
        assert_eq!(text(kk_document_format(doc, &mut len), len), "docx");
        assert_eq!(kk_document_page_count(doc), 0);
        assert_eq!(kk_document_warning_count(doc), 0);
        assert!(kk_document_warning(doc, 0, ptr::null_mut()).is_null());

        let target = CString::new("markdown").unwrap();
        let out = kk_render(doc, target.as_ptr(), ptr::null(), &mut err);
        assert!(!out.is_null() && err.is_null());
        assert!(text(kk_output_text(out, &mut len), len).contains('#'));
        assert!(kk_output_data(out, ptr::null_mut()).is_null());
        for index in 0..kk_output_file_count(out) {
            assert!(text(kk_output_file_path(out, index, &mut len), len).starts_with("images/"));
            assert!(!kk_output_file_data(out, index, &mut len).is_null());
        }
        kk_output_free(out);

        let target = CString::new("docx").unwrap();
        let out = kk_render(doc, target.as_ptr(), ptr::null(), ptr::null_mut());
        let data = kk_output_data(out, &mut len);
        assert!(len > 1000 && std::slice::from_raw_parts(data, 2) == b"PK");
        assert!(kk_output_text(out, ptr::null_mut()).is_null());
        kk_output_free(out);
        kk_document_free(doc);
    }
}

#[test]
fn errors_have_a_kind_and_a_message() {
    unsafe {
        let mut err = ptr::null_mut();
        let doc = kk_open_data(b"nothing".as_ptr(), 7, ptr::null(), &mut err);
        assert!(doc.is_null());
        assert_eq!(kk_error_kind(err), KK_ERROR_DOCUMENT);
        kk_error_free(err);

        let doc = kk_open_path(sample("word/sample.docx").as_ptr(), ptr::null(), &mut err);
        let target = CString::new("epub").unwrap();
        let opts = CString::new(r#"{"imageDir":"x"}"#).unwrap();
        let out = kk_render(doc, target.as_ptr(), opts.as_ptr(), &mut err);
        assert!(out.is_null());
        assert_eq!(kk_error_kind(err), KK_ERROR_INVALID_ARGUMENT);
        let mut len = 0;
        assert_eq!(
            text(kk_error_message(err, &mut len), len),
            "imageDir does not apply to epub"
        );
        kk_error_free(err);

        // NULL arguments are errors, not crashes; NULL frees do nothing.
        let out = kk_render(ptr::null(), target.as_ptr(), ptr::null(), &mut err);
        assert!(out.is_null());
        assert_eq!(kk_error_kind(err), KK_ERROR_INVALID_ARGUMENT);
        kk_error_free(err);
        assert!(kk_open_path(ptr::null(), ptr::null(), ptr::null_mut()).is_null());
        kk_document_free(doc);
        kk_document_free(ptr::null_mut());
        kk_output_free(ptr::null_mut());
        kk_error_free(ptr::null_mut());
    }
}

#[test]
fn a_document_renders_from_several_threads() {
    struct Shared(*mut KkDocument);
    // SAFETY: a KkDocument can be rendered from several threads (the contract).
    unsafe impl Sync for Shared {}
    impl Shared {
        /// The document (a method, so that closures capture the whole
        /// `Shared`, not its raw pointer).
        fn get(&self) -> *mut KkDocument {
            self.0
        }
    }
    let doc = unsafe { kk_open_path(sample("word/sample.docx").as_ptr(), ptr::null(), ptr::null_mut()) };
    let shared = Shared(doc);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| unsafe {
                let target = CString::new("json").unwrap();
                let out = kk_render(shared.get(), target.as_ptr(), ptr::null(), ptr::null_mut());
                assert!(!kk_output_text(out, ptr::null_mut()).is_null());
                kk_output_free(out);
            });
        }
    });
    unsafe { kk_document_free(doc) };
}
