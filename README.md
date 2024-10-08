# Rusty Pinterest Scraper 🦀

A Pinterest scraper built with Rust.

## Description

This project is a Pinterest scraper implemented in Rust. It allows users to extract data from Pinterest boards and pins efficiently.

## Features

- Rapid scraping of Pinterest images 🗣️🔊🔥
- Support for CSV file input of search terms 😎
- Multi-threaded downloading for improved performance 📈
- Real-time progress display with scraping speed 👀
- Flexible searching with optional search suffix ✒️🔏

## Installation

```bash
git clone https://github.com/your-username/rusty_pinterest_scraper.git
cd rusty_pinterest_scraper
cargo build
```

## Usage

```bash
cargo run -- [arguments]
cargo run -- --username <your_pinterest_username> --password <your_pinterest_password> --csv-file <path_to_csv>
cargo run -- --username <your_pinterest_username> --password <your_pinterest_password> --url <url>
```
```csv
name,max_images
"raiden shogun",100
"Sparkle",50
"ellen joe",
```
in the max_images, if you didnt enter the number. it will scroll for an inf

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

Copyright (c) 2024 SteelBolt
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.
Attribution to the original creator (SteelBolt) must be provided in any substantial use or distribution of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
