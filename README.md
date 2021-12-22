# my-esp-idf
My very opinionated ESP-IDF rust library.

This is experimental software. Rust libraries for the ESP32 are highly volatile right now, so expect stuff to break often.

Features:

* Limited BLE support (discovery, write and notifications supported).
* Concurrent BLE and Wifi connections.
* Steam Controller BLE support (client).
* Basic servo controller.
* L298 motor driver (with speed control) controller.

How to use:

1. First get a [rust ESP32 project](https://github.com/esp-rs/esp-idf-template/blob/master/README-cmake.md) working.
2. Add / change dependencies on Cargo.toml ```[dependencies]```
```
[dependencies]
esp-idf-sys = { version = "0.30.4", features = ["native"] }
esp-idf-hal = "0.32.5"
esp-idf-svc = "0.36.7"
my-esp-idf = { git = "https://github.com/lkoba/my-esp-idf" }
```
3. Add patched dependencies to Cargo.toml ```[patch.crates-io]```
```
[patch.crates-io]
esp-idf-sys = { git = "https://github.com/lkoba/esp-idf-sys" }
esp-idf-hal = { git = "https://github.com/lkoba/esp-idf-hal" }
esp-idf-svc = { git = "https://github.com/lkoba/esp-idf-svc" }
```
4. Build and flash like explained on the website linked in point 1.

You can also check my [rusted-rover project](https://github.com/lkoba/rusted-rover) to see this lib in use.
