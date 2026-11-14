//! A Markdown viewer on Div.
//!
//! ```text
//! markdown-viewer [--theme light|dark] [--png OUT.png] [--scroll-to PX] [FILE.md]
//! ```
//!
//! Built with the `linux` or `windows` feature, it opens a window with an
//! Open button (a native file dialog), a theme switch and the document in a
//! scroll container; the wheel and the scroll bar scroll it. `--png`, or
//! `DIV_RENDER_PNG` as for any Div app, renders the first frame instead.
//! Built without them, it renders headlessly and `--png` defaults to
//! `viewer.png`.
//!
//! What a browser would do and Div cannot yet is labeled `div-gap` in the
//! code and listed in `docs/framework-gaps.md`: link activation (MD-17),
//! the system theme and the package's colors (MD-4, MD-5, MD-6), image
//! sizes (MD-15), the elements Div lacks (MD-1) and the colors of
//! highlighted code, which are in `style` attributes (MD-24).

mod viewer;

use std::error::Error;
use std::path::PathBuf;

use div::{Document, EngineOptions, ViewportMetrics};
use viewer::{Theme, Viewer};

const SIZE: (f64, f64) = (900.0, 700.0);

struct Arguments {
    theme: Theme,
    png: Option<PathBuf>,
    scroll_to: f64,
    file: Option<PathBuf>,
}

fn arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut parsed = Arguments {
        theme: Theme::Light,
        png: None,
        scroll_to: 0.0,
        file: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--theme" => {
                parsed.theme = match value()?.as_str() {
                    "light" => Theme::Light,
                    "dark" => Theme::Dark,
                    other => return Err(format!("unknown theme {other}").into()),
                }
            }
            "--png" => parsed.png = Some(PathBuf::from(value()?)),
            "--scroll-to" => parsed.scroll_to = value()?.parse()?,
            _ if arg.starts_with("--") => return Err(format!("unknown option {arg}").into()),
            _ => parsed.file = Some(PathBuf::from(arg)),
        }
    }
    Ok(parsed)
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = arguments()?;
    let document = Document::with_options(
        ViewportMetrics::from_logical_size(SIZE.0, SIZE.1, 1.0)?,
        EngineOptions::default(),
    )?;
    let windowed = cfg!(any(feature = "linux", feature = "windows")) && arguments.png.is_none();
    let viewer = Viewer::new(&document, windowed)?;
    viewer.set_theme(arguments.theme)?;
    match &arguments.file {
        Some(file) => viewer.open(file)?,
        None => viewer.show_status("Open a Markdown file."),
    }
    document.update_all()?;
    viewer.scroller.scroll_to(0.0, arguments.scroll_to)?;
    if !windowed {
        let png = arguments.png.unwrap_or_else(|| PathBuf::from("viewer.png"));
        document.render_to_png(&png)?;
        println!("wrote {}", png.display());
        return Ok(());
    }
    run(document, viewer)
}

#[cfg(any(feature = "linux", feature = "windows"))]
fn run(document: Document, viewer: std::rc::Rc<Viewer>) -> Result<(), Box<dyn Error>> {
    use div::app::{App, LogicalSize, WindowOptions};
    use div::dialog;
    use div::task::spawn_local;

    if let Some(open) = &viewer.open_button {
        let weak = std::rc::Rc::downgrade(&viewer);
        open.on("click", move |_| {
            let chosen = dialog::open_file()
                .title("Open a Markdown file")
                .filter("Markdown", &["md", "markdown"])
                .filter("All files", &[])
                .pick();
            let weak = weak.clone();
            spawn_local(async move {
                let Some(viewer) = weak.upgrade() else {
                    return;
                };
                match chosen.await {
                    Ok(Some(path)) => {
                        if let Err(error) = viewer.open(&path) {
                            viewer.show_status(&format!("Could not open it: {error}"));
                        }
                    }
                    Ok(None) => {}
                    Err(error) => viewer.show_status(&error.to_string()),
                }
            })
            .detach();
        })?;
    }
    let title = match viewer.path() {
        Some(path) => format!("{} – Markdown on Div", path.display()),
        None => "Markdown on Div".to_owned(),
    };
    let options = WindowOptions {
        title,
        size: LogicalSize::new(SIZE.0, SIZE.1),
        ..WindowOptions::default()
    };
    App::from_document(document, options)?.run_document()?;
    Ok(())
}

#[cfg(not(any(feature = "linux", feature = "windows")))]
fn run(_: Document, _: std::rc::Rc<Viewer>) -> Result<(), Box<dyn Error>> {
    unreachable!("without a platform feature the viewer renders a PNG")
}
