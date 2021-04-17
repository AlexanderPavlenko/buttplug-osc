# buttplug-osc

Thanks to [buttplug.io](https://buttplug.io/), this program allows to control
~~numerous supported devices~~ at least ones which I have via [OSC](https://en.wikipedia.org/wiki/Open_Sound_Control).

## Usage

```shell
buttplug-osc 0.1.0
Control https://buttplug.io/ devices via OSC

USAGE:
    buttplug-osc [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --intiface-connect <intiface-connect>     [default: ws://127.0.0.1:12345]
        --osc-listen <osc-listen>                 [default: udp://0.0.0.0:9000]
        --log-level <rust-log>                    [env: RUST_LOG=]  [default: debug]
```

### Supported OSC messages

* /devices/`<name>`/`<command>`/`<argument>`

  * Device `<name>`
    * full name as in the log output: `INFO buttplug_osc: [XBoxXInputCompatibleGamepad] added`
    * `<name>` as a prefix; may be used to address the multiple devices or ones with a very long name
    * `last` is an alias for the recently (re)connected device
    * `all` is an alias for all connected devices
  * Command `vibrate`
    * Argument `speed`: from 0.0 to 1.0 ([details](https://docs.rs/buttplug/3.0.0/buttplug/client/device/enum.VibrateCommand.html#variant.Speed))
  * Command `stop`

## Features

* Reconnects if device or server temporarily disconnected
* OSC receiver
* Control multiple devices
* [Build for Windows 10](https://github.com/AlexanderPavlenko/buttplug-osc/releases/latest/download/buttplug-osc.exe)

## TODO

* Demo patches for:
    * VCV Rack
    * TC-Data
    * Ableton Live 11
