use xiv_gen::Language;
fn main() {
    for (lang, data) in xiv_gen_db::all_locales() {
        for (id, item) in &data.items {
            if item.name.chars().count() > 70 {
                println!("{:?}: ID={}, Name={}, Length={}", lang, id.0, item.name, item.name.chars().count());
            }
        }
    }
}
