# pomctl

A pomodoro timer for the terminal. It counts down in block digits, cycles
through work and breaks on its own, and sends a notification when a phase is over.

```
                            WORK 1/4

             ▄▄▄▄      ▄▄▄           ▄▄▄       ▄▄▄▄
            ██▀▀██    █▀██     ██   █▀██      ██▀▀▀█
           ██    ██     ██     ▀▀     ██     ██ ▄▄▄
           ██ ██ ██     ██            ██     ███▀▀██▄
           ██    ██     ██            ██     ██    ██
            ██▄▄██   ▄▄▄██▄▄▄  ██  ▄▄▄██▄▄▄  ▀██▄▄██▀
             ▀▀▀▀    ▀▀▀▀▀▀▀▀  ▀▀  ▀▀▀▀▀▀▀▀    ▀▀▀▀

                ████████████░░░░░░░░░░░░░░░░░░░░

                 space pause · s skip · q quit
```

## Install

```sh
cargo install pomctl
```

## Use

```sh
pomctl              # 25 work, 5 break, 15 long break
pomctl 50           # 50 minute work phases, breaks unchanged
pomctl 50 10 20     # work, break, long break
```

Durations are in minutes and positional. A long break is taken after every
fourth work phase, and the cycle then starts over. It runs until you quit.

| Key             | Does                                             |
| --------------- | ------------------------------------------------ |
| `space`         | pause or resume                                  |
| `s`             | skip to the next phase, without notifying        |
| `q` or `Ctrl-C` | quit, printing what the session came to          |

```
$ pomctl
3 pomodoros completed — 1h 22m focused
```

A pomodoro is a work phase that ran all the way down; focused time is time
actually spent working, with pauses and breaks taken out.

## Notifications

Each phase change calls `notify-send` and plays a sound with `paplay`:

```sh
notify-send -u normal -t 5000 -i utilities-terminal pomctl 'Break time — 5 min'
paplay /usr/share/sounds/freedesktop/stereo/complete.oga
```

Being called back to work is sent as `critical` so do not disturb rules does
not hold them back. Both commands are optional at runtime: if they are not
installed the timer runs on in silence.

## Piped output

With stdout redirected there is no screen to take over and no keyboard to read,
so pomctl prints a line per phase instead and runs until it is killed:

```
$ pomctl > session.log
work 25:00
break 05:00
work 25:00
```

## Building on it

`src/render/font.txt` is the block font.
To change a digit, paste over it; `cargo test` checks that every glyph is the
same height and that nothing a clock needs is missing.

Btw the font is the same as other project of mine:
[faster-cli](https://github.com/CfM47/faster-cli).
