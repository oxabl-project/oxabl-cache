//! The window as a person uses it: what it shows, and pressing, pasting and typing into it.

use std::time::{Duration, Instant};

use oxabl_cache_desktop::data::paths::Paths;
use oxabl_cache_desktop::data::worker::Worker;
use oxabl_cache_desktop::ui::state::Ui;
use oxabl_cache_desktop::{SIZE, ui};
use rune::clipboard::MemoryClipboard;
use rune::{Env, Key, KeyEvent, Runtime};

const FRAME: f64 = 1.0 / 60.0;
const PATIENCE: Duration = Duration::from_secs(45);

pub struct Window {
    rt: Runtime<Ui>,
    pub clipboard: MemoryClipboard,
}

impl Window {
    /// The window as it opens on the person's computer, with its background running. The
    /// tests bring their own credentials: one in the environment would take precedence, as it
    /// does for a person, and point the window at another server.
    pub fn open(paths: &Paths) -> Window {
        if std::env::var_os("OXABL_DESKTOP_LIVE_REPO").is_none() {
            for name in [
                "OXABL_CACHE_TOKEN",
                "OXABL_CACHE_IDENTITY",
                "OXABL_CACHE_SERVER",
            ] {
                assert!(
                    std::env::var_os(name).is_none(),
                    "unset {name}: the acceptance tests use credentials of their own"
                );
            }
        }
        let mut window = Window::open_without_network(paths);
        let worker = Worker::start(paths.clone(), None);
        window.rt.state.connect(worker);
        window
    }

    /// The window's first frame, before its background has answered anything.
    pub fn open_without_network(paths: &Paths) -> Window {
        let ui = Ui::load(paths.clone());
        let mut rt = Runtime::new(
            ui,
            ui::view::view,
            SIZE,
            Env {
                system_dark: true,
                scale: 1.0,
            },
        );
        rt.poll = Some(ui::poll);
        rt.on_key = Some(ui::on_key);
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        rt.frame(0.0);
        Window { rt, clipboard }
    }

    fn step(&mut self) {
        self.rt.frame(FRAME);
    }

    fn settle(&mut self) {
        for _ in 0..40 {
            self.step();
        }
        self.rt.settle_all();
        self.step();
    }

    /// Every text the window shows now.
    pub fn texts(&self) -> Vec<String> {
        self.rt
            .elements()
            .into_iter()
            .filter(|e| !e.exiting && e.opacity > 0.01)
            .filter_map(|e| e.text.or(e.label))
            .collect()
    }

    pub fn shows(&self, text: &str) -> bool {
        self.texts().iter().any(|t| t.contains(text))
    }

    /// The text of the element `key`.
    pub fn text_of(&self, key: &str) -> Option<String> {
        self.rt
            .elements()
            .into_iter()
            .filter(|e| !e.exiting)
            .find(|e| e.key.as_deref() == Some(key))
            .and_then(|e| e.text.or(e.label))
    }

    /// The key of a cache's row on the home screen.
    pub fn card_key(cache: &str) -> String {
        format!("card-{cache}")
    }

    pub fn has(&self, key: &str) -> bool {
        self.rt
            .elements()
            .into_iter()
            .any(|e| !e.exiting && e.key.as_deref() == Some(key))
    }

    /// Runs the window until `done` holds, in real time, so the background can answer.
    pub fn wait_until(&mut self, what: &str, done: impl Fn(&Window) -> bool) {
        let started = Instant::now();
        while !done(self) {
            assert!(
                started.elapsed() < PATIENCE,
                "waited {PATIENCE:?} for {what}; the window shows {:?}",
                self.texts()
            );
            self.step();
            std::thread::sleep(Duration::from_millis(15));
        }
        self.settle();
    }

    pub fn wait_for(&mut self, text: &str) {
        let wanted = text.to_owned();
        self.wait_until(&format!("\"{text}\""), move |w| w.shows(&wanted));
    }

    /// Presses the element `key` as a pointer would.
    pub fn press(&mut self, key: &str) {
        self.settle();
        let (x, y) = self
            .rt
            .find(key)
            .unwrap_or_else(|| panic!("no {key} on screen; the window shows {:?}", self.texts()));
        self.rt.pointer_down(x, y);
        self.rt.pointer_up(x, y);
        self.settle();
    }

    /// Focuses the field `key` and pastes `text` into it, as a person pasting with the keyboard.
    pub fn paste_into(&mut self, key: &str, text: &str) {
        self.press(key);
        self.rt.paste(text);
        self.settle();
    }

    pub fn key(&mut self, key: Key) {
        self.rt.key(KeyEvent {
            key,
            mods: Default::default(),
        });
        self.settle();
    }

    /// What the window put on the clipboard last.
    pub fn copied(&self) -> Option<String> {
        self.clipboard.text().map(|t| t.to_string())
    }

    /// The window's background colour, to tell light from dark.
    pub fn backdrop_luminance(&self) -> f32 {
        self.rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("window"))
            .map(|e| e.fill.luminance())
            .expect("the window")
    }
}
