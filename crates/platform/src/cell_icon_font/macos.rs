use std::ffi::{CStr, CString, c_char, c_void};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr;

type CfRef = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(allocator: CfRef, bytes: *const c_char, encoding: u32) -> CfRef;
    fn CFDictionaryCreate(
        allocator: CfRef,
        keys: *const CfRef,
        values: *const CfRef,
        count: isize,
        key_callbacks: CfRef,
        value_callbacks: CfRef,
    ) -> CfRef;
    fn CFSetCreate(allocator: CfRef, values: *const CfRef, count: isize, callbacks: CfRef)
    -> CfRef;
    fn CFArrayGetCount(array: CfRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CfRef, index: isize) -> CfRef;
    fn CFURLGetFileSystemRepresentation(
        url: CfRef,
        absolute: bool,
        buffer: *mut u8,
        size: isize,
    ) -> bool;
    fn CFRelease(value: CfRef);
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    static kCTFontFamilyNameAttribute: CfRef;
    static kCTFontURLAttribute: CfRef;
    fn CTFontDescriptorCreateWithAttributes(attributes: CfRef) -> CfRef;
    fn CTFontDescriptorCreateMatchingFontDescriptors(
        descriptor: CfRef,
        mandatory_attributes: CfRef,
    ) -> CfRef;
    fn CTFontDescriptorCopyAttribute(descriptor: CfRef, attribute: CfRef) -> CfRef;
}

struct OwnedCf(CfRef);

impl OwnedCf {
    fn new(value: CfRef) -> Option<Self> {
        (!value.is_null()).then(|| Self(value))
    }
}

impl Drop for OwnedCf {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

pub(super) fn font_paths(family: &str) -> io::Result<Vec<PathBuf>> {
    let family = CString::new(family)?;
    let error = || io::Error::other("cannot inspect the Core Text font catalog");
    // The dictionary and set borrow the family name and attribute key for this query.
    unsafe {
        let null = ptr::null();
        let family = OwnedCf::new(CFStringCreateWithCString(
            null,
            family.as_ptr(),
            0x0800_0100,
        ))
        .ok_or_else(error)?;
        let key = kCTFontFamilyNameAttribute;
        let attributes = OwnedCf::new(CFDictionaryCreate(null, &key, &family.0, 1, null, null))
            .ok_or_else(error)?;
        let required = OwnedCf::new(CFSetCreate(null, &key, 1, null)).ok_or_else(error)?;
        let descriptor =
            OwnedCf::new(CTFontDescriptorCreateWithAttributes(attributes.0)).ok_or_else(error)?;
        let Some(matches) = OwnedCf::new(CTFontDescriptorCreateMatchingFontDescriptors(
            descriptor.0,
            required.0,
        )) else {
            return Ok(Vec::new());
        };
        let mut paths = Vec::new();
        for index in 0..CFArrayGetCount(matches.0) {
            let descriptor = CFArrayGetValueAtIndex(matches.0, index);
            let url = OwnedCf::new(CTFontDescriptorCopyAttribute(
                descriptor,
                kCTFontURLAttribute,
            ))
            .ok_or_else(error)?;
            let mut bytes = [0_u8; 4096];
            if !CFURLGetFileSystemRepresentation(
                url.0,
                true,
                bytes.as_mut_ptr(),
                bytes.len() as isize,
            ) {
                return Err(error());
            }
            let path = CStr::from_ptr(bytes.as_ptr().cast()).to_bytes();
            paths.push(PathBuf::from(std::ffi::OsStr::from_bytes(path)));
        }
        Ok(paths)
    }
}
