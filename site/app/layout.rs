use askama::Template;

#[derive(Template)]
#[template(path = "layout.html")]
pub struct RootLayout<'a> {
    pub children: &'a str,
}

pub fn render(children: &str) -> String {
    RootLayout { children }.render().unwrap()
}
