
// todo: removeme
#[macro_use]
extern crate serde_derive;

extern crate serde;
extern crate serde_json;

use std::os::raw::*;
use std::ptr::null;
use std::ptr::null_mut;

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
        // it may be easier to suppress wilton_module_init leaks in valgrind
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

pub fn create_wilton_error(error_opt: Option<String>) -> *mut c_char {
    match error_opt {
        Some(msg) => copy_to_wilton_bufer(&msg),
        None => null_mut::<c_char>()
    }
}


// todo: removeme
#[derive(Deserialize)]
struct MyIn {
    bar: i32,
    baz: i32,
}

#[derive(Serialize)]
struct MyOut {
    boo: i32,
    baa: i32,
}

#[no_mangle]
pub extern "C" fn wilton_module_init() -> *mut c_char {

    const NUM1: i32 = 5;

    // register foo call
    let foo_res = register_wiltocall("foo", |obj: MyIn| -> MyOut {
        MyOut { boo: (obj.bar + NUM1), baa: (obj.baz + NUM1) }
    });
    if foo_res.is_err() {
        return create_wilton_error(foo_res.err());
    }
    
    // register bar call
    let bar_res = register_wiltocall("bar", |obj: MyIn| -> MyOut {
        MyOut { boo: (obj.bar - NUM1), baa: (obj.baz - NUM1) }
    });
    if bar_res.is_err() {
        return create_wilton_error(bar_res.err());
    }

    create_wilton_error(None)
}
