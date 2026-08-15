use termotion_schema::theme;

pub fn list() -> i32 {
    println!("builtin:");
    for name in theme::list_builtin_names() {
        println!("  {name}");
    }
    0
}
