use rune::Icon;

const fn stroke(name: &'static str, body: &'static str) -> Icon {
    Icon {
        name,
        view: 24.0,
        body,
    }
}

pub static REFRESH: Icon = stroke(
    "oxc-refresh",
    r#"<path d="M20 11.5A8 8 0 1 0 17.6 17.3" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round"/><path d="M20.5 5V11.5H14" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
pub static PLUS: Icon = stroke(
    "oxc-plus",
    r#"<path d="M12 5V19M5 12H19" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"#,
);
pub static GEAR: Icon = stroke(
    "oxc-gear",
    r#"<circle cx="12" cy="12" r="3" stroke="currentColor" stroke-width="2" fill="none"/><path d="M12 2.5V5M12 19V21.5M21.5 12H19M5 12H2.5M18.7 5.3L17 7M7 17L5.3 18.7M18.7 18.7L17 17M7 7L5.3 5.3" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"#,
);
pub static BACK: Icon = stroke(
    "oxc-back",
    r#"<path d="M15 5L8 12L15 19" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
pub static NEXT: Icon = stroke(
    "oxc-next",
    r#"<path d="M9 5L16 12L9 19" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
pub static COPY: Icon = stroke(
    "oxc-copy",
    r#"<rect x="8" y="8" width="12" height="12" rx="2.5" stroke="currentColor" stroke-width="2" fill="none"/><path d="M16 8V6.5A2.5 2.5 0 0 0 13.5 4H6.5A2.5 2.5 0 0 0 4 6.5V13.5A2.5 2.5 0 0 0 6.5 16H8" stroke="currentColor" stroke-width="2" fill="none"/>"#,
);
pub static CHECK: Icon = stroke(
    "oxc-check",
    r#"<path d="M5 12.5L10 17.5L19 7" stroke="currentColor" stroke-width="2.4" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
pub static BOLT: Icon = stroke(
    "oxc-bolt",
    r#"<path d="M13 2.5L4.5 13.5H11.5L10.5 21.5L19.5 10H12.5L13 2.5Z" stroke="currentColor" stroke-width="2" fill="none" stroke-linejoin="round"/>"#,
);
pub static LOCK: Icon = stroke(
    "oxc-lock",
    r#"<rect x="5" y="10.5" width="14" height="10" rx="2.5" stroke="currentColor" stroke-width="2" fill="none"/><path d="M8 10.5V7.5A4 4 0 0 1 16 7.5V10.5" stroke="currentColor" stroke-width="2" fill="none"/>"#,
);
pub static LINK: Icon = stroke(
    "oxc-link",
    r#"<path d="M14 4H20V10M20 4L11 13M18 14V18.5A1.5 1.5 0 0 1 16.5 20H5.5A1.5 1.5 0 0 1 4 18.5V7.5A1.5 1.5 0 0 1 5.5 6H10" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#,
);
pub static PASTE: Icon = stroke(
    "oxc-paste",
    r#"<rect x="5" y="4.5" width="14" height="16.5" rx="2.5" stroke="currentColor" stroke-width="2" fill="none"/><path d="M9 4.5V3.5H15V6H9V4.5Z" stroke="currentColor" stroke-width="2" fill="none" stroke-linejoin="round"/>"#,
);
