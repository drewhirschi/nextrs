use axum::response::IntoResponse;

// This module is absent from the default server bundle, so `csv` need not
// exist in that bundle's dependency graph. Ordinary local builds enable it.
pub async fn get() -> impl IntoResponse {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(["id", "title", "completed"]).unwrap();
    writer
        .write_record(["1", "Try a separate server bundle", "false"])
        .unwrap();
    (
        [
            ("content-type", "text/csv"),
            ("content-disposition", "attachment; filename=todos.csv"),
        ],
        writer.into_inner().unwrap(),
    )
}
