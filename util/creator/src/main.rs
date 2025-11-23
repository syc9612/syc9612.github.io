use std::env;
use std::fs;
use std::process;

use pulldown_cmark::{Parser, Options, html};

fn main() 
{
    //인자 입력
    let args: Vec<String> = env::args().collect();

    if args.len() != 2
    {
        eprintln!("Usage {} <input.md>", args[0]);
        process::exit(1);
    }

    //html path
    let input_path = &args[1];

    let markdown =
        match fs::read_to_string(input_path)
        {
            Ok(s) => s,
            Err(e) => 
            {
                eprintln!("Fail to read {}: {}", input_path, e);
                process::exit(1);
            }
        };
    
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    
    // md -> html convert
    let parser = Parser::new_ext(&markdown, options);

    let mut html_output = String::new();
    if html::push_html(&mut html_output, parser) == false
    {
        panic!("can not push to html");
    }

    println!("{}", html_output);
}
