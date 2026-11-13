fn main() {
    let path = std::env::args().nth(1).expect("markdown file");
    let source = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
    let nodes = markdown_tree::parse(&source);
    println!("{} top-level nodes", nodes.len());
    println!("{}", markdown_tree::to_html(&nodes));
}
