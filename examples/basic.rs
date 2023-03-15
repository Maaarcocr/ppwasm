use std::{mem, sync::atomic::AtomicBool};

use wasmtime::{Engine, Module, Store, Func, Caller, WasmBacktrace, Instance, Config};

static BACKTRACES: std::sync::Mutex<Vec<wasmtime_runtime::Backtrace>> = std::sync::Mutex::new(vec![]);

fn main() {
    let mut config = Config::default();
    config.epoch_interruption(true);
    let engine = Engine::new(&config).unwrap();
    let module = Module::new(
        &engine,
        r#"
            (module
                (import "" "" (func $host))
                (func $foo (export "f") call $zed)
                (func $bar call $host)
                (func $zed
                    ;; create a local variable and initialize it to 0
                    (local $i i32)
                
                    (loop $my_loop
                
                      ;; add one to $i
                      local.get $i
                      i32.const 1
                      i32.add
                      local.set $i
                
                      call $bar
                
                      ;; if $i is less than 10 branch to loop
                      local.get $i
                      i32.const 10
                      i32.lt_s
                      br_if $my_loop
                    )
                  )
            )
        "#,
    ).unwrap();

    let mut store = Store::new(&engine, ());
    let func = Func::wrap(&mut store, |_: Caller<'_, ()>| {
        // function that sleeps for 30 seconds
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("Hello from Rust!");
    });
    let instance = Instance::new(&mut store, &module, &[func.into()]).unwrap();
    let func = instance.get_typed_func::<(), ()>(&mut store, "f").unwrap();

    store.set_epoch_deadline(1);

    store.epoch_deadline_callback(|_| {
        BACKTRACES.try_lock().unwrap().push(wasmtime_runtime::Backtrace::new());
        Ok(1)
    });

    let done = AtomicBool::new(false);

    std::thread::scope(|s| {
        s.spawn(|| {
            loop {
                engine.increment_epoch();
                std::thread::sleep(std::time::Duration::from_millis(100));
                if done.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
            }
        });
        // this thread will call `func.call(&mut store, ())` and will be interrupted after 10 seconds
        func.call(&mut store, ()).unwrap();
        done.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    
    let mut backtraces = BACKTRACES.lock().unwrap();
    let backtraces = mem::replace(&mut *backtraces, vec![]);
    let expanded = backtraces.into_iter().map(|bt| WasmBacktrace::from_runtime_backtrace(&store, bt));
    for bt in expanded {
        for frame in bt.frames() {
            println!("  {:?}", frame);
        }
    }
}