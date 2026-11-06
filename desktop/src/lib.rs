//! A small window onto the oxabl caches of your organisations: how big each is, how fast your
//! connection to it is, how long pulls and pushes take and how often a build hits, for the
//! whole team and for this computer, and a three-step way to join one.

pub mod data;
pub mod fx;
pub mod ui;

use rune::{App, AppSpec, Waker, WindowOptions};

use data::paths::Paths;
use data::worker::Worker;
use ui::state::Ui;

pub const TITLE: &str = "Oxabl Cache";
pub const SIZE: (f32, f32) = (520.0, 620.0);
pub const MIN_SIZE: (f32, f32) = (380.0, 460.0);

fn load() -> Ui {
    Ui::load(Paths::from_environment())
}

fn setup(ui: &mut Ui, waker: Waker) {
    if rune::tooling::is_headless() {
        return;
    }
    let worker = Worker::start(ui.paths.clone(), Some(waker));
    ui.connect(worker);
}

/// The app's spec: its view, its poll and keys, and the named scenes headless renders use.
pub fn spec() -> AppSpec<Ui> {
    AppSpec {
        min_size: Some(MIN_SIZE),
        scenes: ui::demo::SCENES,
        setup: Some(setup),
        poll: Some(ui::poll),
        on_key: Some(ui::on_key),
        ..AppSpec::new(TITLE, SIZE, load, ui::view::view)
    }
}

/// Opens the window and runs until it closes.
pub fn run() {
    rune::prewarm_gpu();
    rune::tooling::set_app_id("org.oxabl.Cache");
    App::new(spec())
        .float_on_open()
        .window_options(WindowOptions {
            transparent: true,
            borderless: true,
            min_size: Some(MIN_SIZE),
            ..WindowOptions::default()
        })
        .compositor(fx::Effects::default())
        .run();
}
