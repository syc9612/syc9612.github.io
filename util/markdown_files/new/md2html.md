# md2html

Rust 기반의 md2html 프로그램 만들기.
[`pulldown-cmark`](https://crates.io/crates/pulldown-cmark) 을 활용한다.


---
## 요구사항
 - 멀티스레드로 처리.
 - markdown_files/new/ 에 있는 **모든** MD files를 읽고, html_files에 저장해라.
 - 처리한 md 파일은 markdown_files/save/ 에 저장하여 백업한다.
 - (계획) 차후엔 경로를 입력받아 처리하는쪽이 좋겠다.

---

## 📌 Features Demonstrated


---

## 🦀 Rust Code Example

```rust
fn main() {
    println!("Hello from Rust!");
}
