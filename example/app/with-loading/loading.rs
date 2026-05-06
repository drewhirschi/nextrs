use askama::Template;

#[derive(Template)]
#[template(path = "with-loading/loading.html")]
pub struct WithLoadingLoading;

pub fn render() -> String {
    WithLoadingLoading.render().unwrap()
}
