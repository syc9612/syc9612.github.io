use std::fs;
use std::path::{Path};

use pulldown_cmark::{Parser, Options, html};

fn main() 
{
    //경로 하드코딩
    let input_path = "./markdown_files/new/";
    let output_path = "./html_files/";
    
    //입력받은 경로에서 iter로 돌면서 찾음.
    fs::read_dir(input_path)
        .expect("Cannot read input directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|ext| ext == "md").unwrap_or(false))
        .for_each(|path|
        {
        print!("Doing Converting...");
        if convert_to_html(&path, output_path) == false
        {
            panic!("can not convert_to_html");
        }
        move_to_save(&path);
        });
}

fn convert_to_html(input_file: &Path, output_dir: &str) -> bool
{
    //TO-DO: Converting 하기 전에 html로 만들어지지 않았는지 검사해보자.
    let file_name = input_file.file_stem().unwrap().to_string_lossy();
    let output_path = format!("{}/{}.html", output_dir, file_name);

    if Path::new(&output_path).exists() 
    {
        println!("Already converted : {:?}", output_path);
        return false;
    }

    let markdown = 
    match fs::read_to_string(input_file) 
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Fail to read {:?}: {}", input_file, e);
            return false;
        }
    };

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(&markdown, options);

    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);


    // HTML 파일 쓰기
    if let Err(e) = fs::write(&output_path, html_output) {
        eprintln!("Fail to write {}: {}", output_path, e);
        return false;
    }

    println!("Done: {}", output_path);

    true
}

fn move_to_save(path: &Path)
{
    let file_name = path.file_name().unwrap().to_string_lossy();
    let dst = format!("{}/{}", "./markdown_files/save/", file_name);

    if let Err(e) = std::fs::rename(&path, &dst) 
    {
        eprintln!("Rename failed, trying copy-remove: {e}");

        std::fs::copy(&path, &dst).unwrap();
        std::fs::remove_file(&path).unwrap();
    }

    println!("Moved to {dst}");
}
