use askama::Template;

#[derive(Template)]
#[template(path = "with-layout/layout.html")]
pub struct WithLayoutLayout<'a> {
    pub children: &'a str,
}

pub fn render(children: &str) -> String {
    WithLayoutLayout { children }.render().unwrap()
}
