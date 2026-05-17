use askama::Template;

#[derive(Template)]
#[template(path = "with-layout/loading.html")]
pub struct WithLayoutLoading;

pub fn render() -> String {
    WithLayoutLoading.render().unwrap()
}
