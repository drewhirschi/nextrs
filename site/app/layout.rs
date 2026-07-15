use askama::Template;

#[derive(Template)]
#[template(path = "layout.html")]
pub struct RootLayout<'a> {
    pub children: &'a str,
    pub style_url: &'static str,
}

pub fn render(children: &str) -> String {
    RootLayout {
        children,
        style_url: env!("NEXTRS_STYLE_URL"),
    }
    .render()
    .unwrap()
}
