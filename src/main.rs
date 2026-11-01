//! SystemMonitor: a low-overhead macOS system monitor built on gpui-kit.

mod diagnostics;
mod health;
mod ioreport;
mod logs;
mod mac;
mod sampler;
mod ui;
mod util;

use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;

actions!(system_monitor, [Quit]);

#[derive(Clone, PartialEq, serde::Deserialize, Action)]
#[action(no_json)]
struct SelectTab {
    index: usize,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let start_tab = match args.as_slice() {
        [] => ui::Tab::default(),
        [help] if help == "--help" || help == "-h" => {
            println!(
                "SystemMonitor — macOS system monitor\n\nUsage:\n  SystemMonitor [--tab overview|cpu|gpu|memory|disk|network|apps|power|logs]\n  SystemMonitor --diagnostics SECONDS    Emit 1–3600 live NDJSON samples, without a GUI\n  SystemMonitor --dump-ioreport          Inspect optional power channels\n\nShortcuts: Cmd+1…9 switches tabs; Cmd+Q or Cmd+W quits."
            );
            return;
        }
        [flag] if flag == "--dump-ioreport" => {
            if let Some(mut r) = ioreport::IoReport::new() {
                r.dump();
                std::thread::sleep(std::time::Duration::from_secs(1));
                println!("{:#?}", r.sample());
            } else {
                eprintln!("IOReport is unavailable on this Mac");
            }
            return;
        }
        [flag, seconds] if flag == "--diagnostics" => {
            match seconds.parse::<u64>() {
                Ok(n @ 1..=3600) => diagnostics::run(n),
                _ => {
                    eprintln!("SECONDS must be an integer between 1 and 3600");
                    std::process::exit(2);
                }
            }
            return;
        }
        [flag, tab] if flag == "--tab" => ui::Tab::from_name(tab).unwrap_or_else(|| {
            eprintln!("Unknown tab: {tab}. Use --help for available tabs.");
            std::process::exit(2);
        }),
        _ => {
            eprintln!("Invalid arguments. Use --help for usage.");
            std::process::exit(2);
        }
    };

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("cmd-w", Quit, None),
            ]);
            cx.bind_keys(
                [
                    "cmd-1", "cmd-2", "cmd-3", "cmd-4", "cmd-5", "cmd-6", "cmd-7", "cmd-8", "cmd-9",
                ]
                .into_iter()
                .enumerate()
                .map(|(index, key)| KeyBinding::new(key, SelectTab { index }, None)),
            );
            cx.on_action(|_: &Quit, cx: &mut App| {
                logs::stop();
                cx.quit();
            });
            cx.on_window_closed(|cx, _| {
                logs::stop();
                cx.quit();
            })
            .detach();

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1240.), px(860.)), cx)),
                window_min_size: Some(size(px(860.), px(560.))),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, move |window, cx| {
                window.activate_window();
                window.set_window_title("System Monitor");
                Theme::change(ThemeMode::Dark, Some(window), cx);
                cx.new(|cx| ui::Monitor::new(start_tab, window, cx))
            })
            .expect("failed to open window");
        });
}
