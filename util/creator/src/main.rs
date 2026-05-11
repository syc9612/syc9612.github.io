use std::fs;
use std::path::{Path};
use anyhow::{Result, Context, anyhow};
use pulldown_cmark::{Parser, Options, html};

fn main() -> Result<()> 
{
    let input_path = "../markdown_files/new/";
    let output_path = "../html_files/";

    fs::create_dir_all("../markdown_files/save/")
        .context("Failed to create save directory")?;

    fs::create_dir_all(output_path)
        .context("Failed to create output directory")?;

    fs::read_dir(input_path)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| is_markdown(p))
        .try_for_each(|path| -> Result<()> {
            convert_to_html(&path, output_path)?;
            move_to_save(&path)?;
            Ok::<(), anyhow::Error>(()) 
        })?;

    Ok(())
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

fn convert_to_html(input_file: &Path, output_dir: &str) -> Result<()> {
    // 파일명 안전하게 얻기
    let file_stem = input_file
        .file_stem()
        .and_then(|x| x.to_str())
        .ok_or_else(|| anyhow!("Invalid UTF-8 filename"))?;

    let output_path = format!("{}/{}.html", output_dir, file_stem);

    // 이미 만들어진 HTML 파일이 있으면 변환하지 않음
    if Path::new(&output_path).exists() {
        return Err(anyhow!("HTML already exists: {}", output_path));
    }

    // Markdown 읽기
    let markdown = fs::read_to_string(input_file)
        .with_context(|| format!("Fail to read {:?}", input_file))?;

    // 변환 옵션
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    // Markdown → HTML 변환
    let parser = Parser::new_ext(&markdown, options);

    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    // HTML 파일 쓰기
    fs::write(&output_path, html_output)
        .with_context(|| format!("Fail to write {}", output_path))?;

    println!("Done: {}", output_path);

    Ok(())
}

fn move_to_save(path: &Path) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|x| x.to_str())
        .ok_or_else(|| anyhow!("Invalid file name"))?;

    let dst = format!("../markdown_files/save/{}", file_name);

    // rename → 실패하면 copy/remove fallback
    if let Err(e) = fs::rename(path, &dst) {
        eprintln!("Rename failed ({e}), trying copy-remove fallback");

        fs::copy(path, &dst)
            .with_context(|| format!("copy failed {:?}", path))?;

        fs::remove_file(path)
            .with_context(|| format!("remove failed {:?}", path))?;
    }

    println!("Moved to {}", dst);
    Ok(())
}
