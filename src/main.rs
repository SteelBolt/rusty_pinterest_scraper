use clap::Parser;
use csv::Reader;
use std::fs;
use thirtyfour::prelude::*;
use anyhow::Result;
use std::time::Duration;
use serde::Deserialize;
use sanitize_filename::sanitize;
use urlencoding;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::interval;
use std::io::{stdout, Write};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute,
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::collections::HashSet;
use regex::Regex;
use indicatif::{ProgressBar, ProgressStyle};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    csv_file: Option<String>,

    #[clap(short, long)]
    username: String,

    #[clap(short, long)]
    password: String,

    #[clap(long)]
    url: Option<String>,

    #[clap(short, long, default_value = "")]
    search_suffix: String,

    #[clap(long)]
    max_images: Option<usize>,

    #[clap(long, default_value_t = 4)]
    threads: usize,

    #[clap(long, default_value = "http://localhost:9515")]
    chromedriver_url: String,
}

#[derive(Debug, Deserialize)]
struct Pin {
    image_url: String,
}

#[derive(Debug, serde::Deserialize)]
struct Character {
    name: String,
}

async fn login(driver: &WebDriver, username: &str, password: &str) -> Result<()> {
    driver.goto("https://www.pinterest.com/login/").await?;
    
    let email_input = driver.find(By::Id("email")).await?;
    let password_input = driver.find(By::Id("password")).await?;
    
    email_input.send_keys(username).await?;
    password_input.send_keys(password).await?;
    
    let login_button = driver.find(By::Css("button[type='submit']")).await?;
    login_button.click().await?;
    
    // Wait for login to complete
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    Ok(())
}

async fn scroll_and_scrape(driver: &WebDriver, url: &str, counter: Arc<AtomicUsize>, max_images: Option<usize>) -> Result<Vec<Pin>> {
    driver.goto(url).await?;

    let mut pins = Vec::new();
    let mut unique_urls = HashSet::new();
    let mut last_height = driver.execute("return document.body.scrollHeight", vec![])
        .await?
        .json()
        .as_u64()
        .unwrap_or(0);

    // Compile regex patterns
    let profile_pic_regex = Regex::new(r"/75x75_RS|/280x280_RS").unwrap();
    let size_regex = Regex::new(r"/\d+x/").unwrap();

    let mut consecutive_empty_scrolls = 0;
    const MAX_EMPTY_SCROLLS: usize = 5;

    let progress_bar = ProgressBar::new(max_images.map(|m| m as u64).unwrap_or(u64::MAX));
    progress_bar.set_style(ProgressStyle::default_bar()
        .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
        .progress_chars("##-"));

    loop {
        let pins_before = pins.len();

        // Scroll down
        driver.execute("window.scrollTo(0, document.body.scrollHeight)", vec![])
            .await?;

        // Wait for new images to load
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Wait for image elements to be present
        let _ = driver.find(By::Css("img[src]")).await?;

        // Extract pins
        let pin_elements = driver.find_all(By::Css("img[src]")).await?;
        for element in pin_elements {
            if let Ok(Some(image_url)) = element.attr("src").await {
                // Skip profile pictures
                if profile_pic_regex.is_match(&image_url) {
                    continue;
                }

                // Get the highest resolution version of the image
                let high_res_url = size_regex.replace(&image_url, "/originals/").to_string();

                // Only add unique URLs
                if unique_urls.insert(high_res_url.clone()) {
                    pins.push(Pin { image_url: high_res_url });
                    counter.fetch_add(1, Ordering::SeqCst);

                    println!("Found new image: {} (Total: {})", pins.len(), unique_urls.len());

                    if let Some(max) = max_images {
                        if pins.len() >= max {
                            println!("Reached maximum number of images: {}", max);
                            progress_bar.finish_with_message(format!("Reached maximum of {} images", max));
                            return Ok(pins);
                        }
                    }
                }
            }
        }

        // Update progress bar
        progress_bar.set_position(pins.len() as u64);
        progress_bar.set_message(format!("Unique images found: {}", unique_urls.len()));

        // Check if we found any new pins in this scroll
        if pins.len() == pins_before {
            consecutive_empty_scrolls += 1;
            println!("No new images found. Empty scroll count: {}", consecutive_empty_scrolls);
        } else {
            consecutive_empty_scrolls = 0;
        }

        // Check if scrolled to the bottom or if we've had too many empty scrolls
        let new_height = driver.execute("return document.body.scrollHeight", vec![])
            .await?
            .json()
            .as_u64()
            .unwrap_or(0);
        if new_height == last_height || consecutive_empty_scrolls >= MAX_EMPTY_SCROLLS {
            println!("Scraping completed. Total unique images found: {}", unique_urls.len());
            break;
        }
        last_height = new_height;
    }

    progress_bar.finish_with_message(format!("Scraping completed. Total unique images found: {}", unique_urls.len()));

    Ok(pins)
}


async fn download_image(client: &reqwest::Client, url: &str, path: &str) -> Result<()> {
    let response = client.get(url).send().await?;
    let bytes = response.bytes().await?;
    fs::write(path, bytes)?;
    println!("Downloaded: {}", path);
    Ok(())
}

async fn display_scrape_progress(counter: Arc<AtomicUsize>, max_images: Option<usize>) {
    let mut interval = interval(Duration::from_millis(100));
    let mut spinner = ['|', '/', '-', '\\'].iter().cycle();
    let start_time = std::time::Instant::now();
    let mut last_count = 0;

    loop {
        interval.tick().await;
        let current_count = counter.load(Ordering::SeqCst);
        let elapsed = start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            let total_speed = current_count as f64 / elapsed;
            let instant_speed = (current_count - last_count) as f64 / 0.1; // 100ms interval
            
            let mut stdout: std::io::Stdout = stdout();
            execute!(
                stdout,
                MoveTo(0, 0),
                Clear(ClearType::CurrentLine)
            ).unwrap();
            
            print!(
                "Progress: {} images{}| Scrape speed: {:.2} pic/sec (Total: {:.2} pic/sec) {} ", 
                current_count,
                max_images.map_or(String::new(), |max| format!("/{} ", max)),
                instant_speed,
                total_speed,
                spinner.next().unwrap()
            );
            stdout.flush().unwrap();
        }
        last_count = current_count;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if args.url.is_none() && args.csv_file.is_none() {
        eprintln!("Error: Either --url or --csv-file must be provided.");
        std::process::exit(1);
    }

    let caps = DesiredCapabilities::chrome();
    let driver = match WebDriver::new(&args.chromedriver_url, caps).await {
        Ok(driver) => driver,
        Err(e) => {
            eprintln!("Failed to connect to ChromeDriver at {}.", args.chromedriver_url);
            eprintln!("Make sure ChromeDriver is running and the port is correct.");
            eprintln!("Check this link if you don't have ChromeDriver: https://googlechromelabs.github.io/chrome-for-testing/#stable");
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // Login
    login(&driver, &args.username, &args.password).await?;

    let counter = Arc::new(AtomicUsize::new(0));
    let client = reqwest::Client::new();

    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, Hide)?;

    let counter_clone = Arc::clone(&counter);
    let max_images = args.max_images;
    tokio::spawn(async move {
        display_scrape_progress(counter_clone, max_images).await;
    });

    if let Some(url) = args.url.as_ref() {
        let sanitized_name = sanitize("pinterest_search");
        fs::create_dir_all(&sanitized_name)?;

        let pins = scroll_and_scrape(&driver, url, Arc::clone(&counter), args.max_images).await?;

        for (i, pin) in pins.iter().enumerate() {
            let file_name = format!("{}/{:03}.jpg", sanitized_name, i + 1);
            download_image(&client, &pin.image_url, &file_name).await?;
        }
    } else if let Some(csv_file) = args.csv_file.as_ref() {
        let mut reader = Reader::from_path(csv_file)?;
        let characters: Vec<Character> = reader.deserialize().collect::<Result<_, csv::Error>>()?;

        for character in characters {
            let sanitized_name = sanitize(&character.name);
            fs::create_dir_all(&sanitized_name)?;

            let search_query = format!("{} {}", character.name, args.search_suffix);
            let url = format!("https://www.pinterest.com/search/pins/?q={}", urlencoding::encode(&search_query));
            
            let pins = scroll_and_scrape(&driver, &url, Arc::clone(&counter), args.max_images).await?;

            for (i, pin) in pins.iter().enumerate() {
                let file_name = format!("{}/{:03}.jpg", sanitized_name, i + 1);
                download_image(&client, &pin.image_url, &file_name).await?;
            }
        }
    }

    driver.quit().await?;

    execute!(stdout, Show, LeaveAlternateScreen)?;

    Ok(())
}