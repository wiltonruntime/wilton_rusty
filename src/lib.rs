/*
 * Copyright 2018, alex at staticlibs.net
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 * http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! Rust modules support for [Wilton JavaScript runtime](https://github.com/wilton-iot/wilton)
//!
//! Usage example:
//!
//! ```
//!// configure Cargo to build a shared library
//![lib]
//!crate-type = ["dylib"]
//! 
//!// in lib.rs, import serde and wilton_rust
//!#[macro_use]
//!extern crate serde_derive;
//!extern crate wilton_rust;
//! ...
//!// declare input/output structs
//!#[derive(Deserialize)]
//!struct MyIn { ... }
//!#[derive(Serialize)]
//!struct MyOut { ... }
//! ...
//!// write a function that does some work
//!fn hello(obj: MyIn) -> MyOut { ... }
//! ...
//!// register that function inside the `wilton_module_init` function,
//!// that will be called by Wilton during the Rust module load
//!#[no_mangle]
//!pub extern "C" fn wilton_module_init() -> *mut std::os::raw::c_char {
//!    // register a call, error checking omitted
//!    wilton_rust::register_wiltocall("hello", |obj: MyIn| { hello(obj) });
//!    // return success status to Wilton
//!    wilton_rust::create_wilton_error(None)
//!}
//!
//! ```
//!
//! See an [example](https://github.com/wilton-iot/wilton_examples/blob/master/rust/test.js#L17)
//! how to load and use Rust library from JavaScript.

extern crate serde;
extern crate serde_json;

use std::os::raw::*;
use std::ptr::null;
use std::ptr::null_mut;


// wilton C API import
// https://github.com/wilton-iot/wilton_core/tree/master/include/wilton

extern "system" {

fn wilton_alloc(
    size_bytes: c_int
) -> *mut c_char;

fn wilton_free(
    buffer: *mut c_char
) -> ();

fn wiltoncall_register(
    call_name: *const c_char,
    call_name_len: c_int,
    call_ctx: *mut c_void,
    call_cb: extern "system" fn(
        call_ctx: *mut c_void,
        json_in: *const c_char,
        json_in_len: c_int,
        json_out: *mut *mut c_char,
        json_out_len: *mut c_int
    ) -> *mut c_char
) -> *mut c_char;

}


static EMPTY_JSON_INPUT: &'static str = "{}";
type WiltonCallback = Box<Fn(&[u8]) -> Result<String, String>>;


// helper functions

fn copy_to_wilton_bufer(data: &str) -> *mut c_char {
    unsafe {
        let res: *mut c_char = wilton_alloc((data.len() + 1) as c_int);
        std::ptr::copy_nonoverlapping(data.as_ptr() as *const c_char, res, data.len());
        *res.offset(data.len() as isize) = '\0' as c_char;
        res
    }
}

fn convert_wilton_error(err: *mut c_char) -> String {
    unsafe {
        use std::ffi::CStr;
        if null::<c_char>() != err {
            let res = match CStr::from_ptr(err).to_str() {
                Ok(val) => String::from(val),
                // generally cannot happen
                Err(_) => String::from("Unknown error")
            };
            wilton_free(err);
            res
        } else {
            // generally cannot happen
            String::from("No error")
        }
    }
}

// https://github.com/rustytools/errloc_macros/blob/79f5378e913293cb1b4a561fb7dc8d5cbcd09bc6/src/lib.rs#L44
fn panicmsg<'a>(e: &'a std::boxed::Box<std::any::Any + std::marker::Send + 'static>) -> &'a str {
    match e.downcast_ref::<&str>() {
        Some(st) => st,
        None => {
            match e.downcast_ref::<std::string::String>() {
                Some(stw) => stw.as_str(),
                None => "()",
            }
        },
    }
}


// callback that is passed to wilton

#[no_mangle]
#[allow(private_no_mangle_fns)]
extern "system" fn wilton_cb(
    call_ctx: *mut c_void,
    json_in: *const c_char,
    json_in_len: c_int,
    json_out: *mut *mut c_char,
    json_out_len: *mut c_int
) -> *mut c_char {
    unsafe {
        std::panic::catch_unwind(|| {
            let data: &[u8] = if (null::<c_char>() != json_in) && (json_in_len > 0) {
                std::slice::from_raw_parts(json_in as *const u8, json_in_len as usize)
            } else {
                EMPTY_JSON_INPUT.as_bytes()
            };
            // https://stackoverflow.com/a/32270215/314015
            let callback_boxed_ptr = std::mem::transmute::<*mut c_void, *mut WiltonCallback>(call_ctx);
            let callback_boxed_ref: &mut WiltonCallback = &mut *callback_boxed_ptr;
            let callback_ref: &mut Fn(&[u8]) -> Result<String, String> = &mut **callback_boxed_ref;
            match callback_ref(data) {
                Ok(res) => {
                    *json_out = copy_to_wilton_bufer(&res);
                    *json_out_len = res.len() as c_int;
                    null_mut::<c_char>()
                }
                Err(e) => copy_to_wilton_bufer(&e)
            }
        }).unwrap_or_else(|e| {
            copy_to_wilton_bufer(panicmsg(&e))
        })
    }
}

/// Registers a closure, that can be called from JavaScript
///
/// This function takes a closure and registers it with Wilton, so
/// it can be called from JavaScript using [wiltoncall](https://wilton-iot.github.io/wilton/docs/html/namespacewiltoncall.html)
/// API.
///
/// Closure must take a single argument - a struct that implements [serde::Deserialize](https://docs.serde.rs/serde/trait.Deserialize.html)
/// and must return a struct that implements [serde::Serialize](https://docs.serde.rs/serde/trait.Serialize.html).
/// Closure input argument is converted from JavaScript object to Rust struct object.
/// Closure output is returned to JavaScript as a JSON (that can be immediately converted to JavaScript object).
///
/// If closure panics, its panic message is converted into JavaScript `Error` message (that can be
/// caugth and handled on JavaScript side).
///
///# Arguments
///
///* `name` - name this call, that should be used from JavaScript to invoke the closure
///* `callback` - closure, that will be called from JavaScript
///
///# Example
///
/// ```
/// // declare input/output structs
///#[derive(Deserialize)]
///struct MyIn { ... }
///#[derive(Serialize)]
///struct MyOut { ... }
/// ...
/// // write a function that does some work
///fn hello(obj: MyIn) -> MyOut { ... }
/// ...
/// // register that function inside the `wilton_module_init` function,
/// // that will be called by Wilton during the Rust module load
///#[no_mangle]
///pub extern "C" fn wilton_module_init() -> *mut std::os::raw::c_char {
///    // register a call, error checking omitted
///    wilton_rust::register_wiltocall("hello", |obj: MyIn| { hello(obj) });
///    // return success status to Wilton
///    wilton_rust::create_wilton_error(None)
///}
///
/// ```
pub fn register_wiltocall<I: serde::de::DeserializeOwned, O: serde::Serialize, F: 'static + Fn(I) -> O>(
    name: &str,
    callback: F
) -> Result<(), String> {
    unsafe {
        use std::error::Error;
        let name_bytes = name.as_bytes();
        let callback_erased = move |json_in: &[u8]| -> Result<String, String> {
            match serde_json::from_slice(json_in) {
                Ok(obj_in) => {
                        let obj_out = callback(obj_in);
                        match serde_json::to_string_pretty(&obj_out) {
                            Ok(json_out) => Ok(json_out),
                            Err(e) => Err(String::from(e.description()))
                        }
                },
                Err(e) => Err(String::from(e.description()))
            }
        };
        let callback_fatty: WiltonCallback = Box::new(callback_erased);
        let callback_slim: Box<WiltonCallback> = Box::new(callback_fatty);
        let callback_bare: *mut WiltonCallback = Box::into_raw(callback_slim);
        // unboxed callbacks are leaked here: 16 byte per callback
        // it seems not easy to make their destructors to run after main
        // https://stackoverflow.com/a/27826181/314015
        // it may be easier to suppress wilton_module_inie leaks in valgrind
        // let callback_unleak = Box::from_raw(callback_bare);

        let err: *mut c_char = wiltoncall_register(
            name_bytes.as_ptr() as *const c_char,
            name_bytes.len() as c_int,
            callback_bare as *mut c_void,
            wilton_cb);

        if null_mut::<c_char>() != err {
            Err(convert_wilton_error(err))
        } else {
            Ok(())
        }
    }
}

/// Create an error message, that can be passed back to Wilton
///
/// Helper function, that can be used with Rust `Result`s, returned
/// from `wilton_rust::register_wiltoncall` function.
///
///# Arguments
///
///* `error_opt` - optional error message, that should be passed back to Wilton
///
///# Example
///
///```
/// // register a call
///let res = wilton_rust::register_wiltocall("hello", |obj: MyObj1| { hello(obj) });
///
/// // check for error
///if res.is_err() {
///    // return error message to Wilton
///    return wilton_rust::create_wilton_error(res.err());
///}
///
/// // return success status to Wilton
///wilton_rust::create_wilton_error(None)
///```
///
pub fn create_wilton_error(error_opt: Option<String>) -> *mut c_char {
    match error_opt {
        Some(msg) => copy_to_wilton_bufer(&msg),
        None => null_mut::<c_char>()
    }
}
