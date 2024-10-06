use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute,
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::io::{stdout, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use thirtyfour::prelude::*;
use tokio::time::{interval, sleep};
use urlencoding;
use serde::Deserialize;

const SCROLL_WAIT_TIME: Duration = Duration::from_secs(5);

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

    #[clap(long, default_value_t = 8)]
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
    max_images: Option<usize>,
}

async fn login(driver: &WebDriver, username: &str, password: &str) -> Result<()> {
    driver.goto("https://www.pinterest.com/login/").await
        .context("Failed to navigate to login page")?;
    
    println!("Navigated to login page");

    let email_input = driver.find(By::Id("email")).await
        .context("Email input not found")?;
    let password_input = driver.find(By::Id("password")).await
        .context("Password input not found")?;
    
    println!("Found email and password inputs");

    email_input.send_keys(username).await
        .context("Failed to input username")?;
    password_input.send_keys(password).await
        .context("Failed to input password")?;
    
    println!("Entered username and password");

    let login_button = driver.find(By::Css("button[type='submit']")).await
        .context("Login button not found")?;
    login_button.click().await
        .context("Failed to click login button")?;
    
    println!("Clicked login button");

    // Wait for login to complete
    sleep(Duration::from_secs(5)).await;
    
    println!("Waited for 5 seconds after clicking login");

    // Check if login was successful
    match driver.find(By::Css("div[data-test-id='header']")).await {
        Ok(_) => {
            println!("Login successful!");
            Ok(())
        },
        Err(_) => {
            println!("Login might have failed. Checking for error messages...");
            if let Ok(error_msg) = driver.find(By::Css("div[data-test-id='error-message']")).await {
                let error_text = error_msg.text().await?;
                Err(anyhow::anyhow!("Login failed: {}", error_text))
            } else {
                Err(anyhow::anyhow!("Login might have failed, but no error message found"))
            }
        }
    }
}

async fn scroll_and_scrape(driver: &WebDriver, url: &str, counter: Arc<AtomicUsize>, max_images: Option<usize>) -> Result<Vec<Pin>> {
    println!("Starting scroll_and_scrape for URL: {}", url);
    driver.goto(url).await.context("Failed to navigate to the URL")?;
    println!("Successfully navigated to the URL");

    let mut pins = Vec::new();
    let mut unique_urls = HashSet::new();
    let mut last_height = driver.execute("return document.body.scrollHeight", vec![])
        .await?
        .json()
        .as_u64()
        .unwrap_or(0);
    println!("Initial page height: {}", last_height);

    // Compile regex patterns
    let profile_pic_regex = Regex::new(r"/75x75_RS|/280x280_RS").unwrap();
    let size_regex = Regex::new(r"/\d+x/").unwrap();

    let progress_bar = ProgressBar::new(max_images.map(|m| m as u64).unwrap_or(u64::MAX));
    progress_bar.set_style(ProgressStyle::default_bar()
        .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
        .progress_chars("##-"));

    loop {
        let pins_before = pins.len();
        println!("Current number of pins: {}", pins_before);

        // Scroll down
        driver.execute("window.scrollTo(0, document.body.scrollHeight)", vec![])
            .await.context("Failed to scroll the page")?;
        println!("Scrolled down");

        // Wait for new images to load
        sleep(SCROLL_WAIT_TIME).await;
        println!("Waited for {} seconds after scrolling", SCROLL_WAIT_TIME.as_secs());

        // Wait for image elements to be present
        match driver.find(By::Css("img[src]")).await {
            Ok(_) => println!("Found image elements"),
            Err(e) => {
                println!("No image elements found: {:?}", e);
                break;
            }
        }

        // Extract pins
        let pin_elements = driver.find_all(By::Css("img[src]")).await.context("Failed to find image elements")?;
        println!("Found {} image elements", pin_elements.len());

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
            println!("No new images found. Scraping completed.");
            break;
        }

        // Check if scrolled to the bottom
        let new_height = driver.execute("return document.body.scrollHeight", vec![])
            .await?
            .json()
            .as_u64()
            .unwrap_or(0);
        println!("New page height: {}", new_height);
        if new_height == last_height {
            println!("Reached the bottom of the page. Scraping completed.");
            break;
        }
        last_height = new_height;
    }

    progress_bar.finish_with_message(format!("Scraping completed. Total unique images found: {}", unique_urls.len()));

    Ok(pins)
}


async fn process_csv_file(driver: &WebDriver, client: &reqwest::Client, csv_file: &str, args: &Args, counter: &Arc<AtomicUsize>) -> Result<()> {
    let mut reader = csv::Reader::from_path(csv_file)?;
    let characters: Vec<Character> = reader.deserialize().collect::<Result<_, csv::Error>>()?;

    for character in characters {
        println!("Processing character: {}", character.name);
        let sanitized_name = sanitize_filename::sanitize(&character.name);
        fs::create_dir_all(&sanitized_name)?;

        let search_query = format!("{} {}", character.name, args.search_suffix);
        let url = format!("https://www.pinterest.com/search/pins/?q={}", urlencoding::encode(&search_query));
       
        // Use character.max_images if available, otherwise fall back to args.max_images
        let max_images = character.max_images.or(args.max_images);
        
        let pins = scroll_and_scrape(driver, &url, Arc::clone(counter), max_images).await?;

        download_images(client, &pins, &sanitized_name).await?;
    }

    Ok(())
}

async fn process_single_url(driver: &WebDriver, client: &reqwest::Client, url: &str, args: &Args, counter: &Arc<AtomicUsize>) -> Result<()> {
    let sanitized_name = sanitize_filename::sanitize("pinterest_search");
    fs::create_dir_all(&sanitized_name)?;

    let pins = scroll_and_scrape(driver, url, Arc::clone(counter), args.max_images).await?;

    download_images(client, &pins, &sanitized_name).await?;

    Ok(())
}

async fn download_images(client: &reqwest::Client, pins: &[Pin], folder: &str) -> Result<()> {
    let progress_bar = ProgressBar::new(pins.len() as u64);
    progress_bar.set_style(ProgressStyle::default_bar()
        .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
        .progress_chars("##-"));

    for (i, pin) in pins.iter().enumerate() {
        let file_name = format!("{}/{:03}.jpg", folder, i + 1);
        download_image(client, &pin.image_url, &file_name).await?;
        progress_bar.inc(1);
        progress_bar.set_message(format!("Downloaded: {}", file_name));
    }

    progress_bar.finish_with_message("All images downloaded");
    Ok(())
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

    println!("Initializing WebDriver...");
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
    println!("WebDriver initialized successfully.");

    // Login
    if let Err(e) = login(&driver, &args.username, &args.password).await {
        eprintln!("Login failed: {:?}", e);
        driver.quit().await?;
        return Err(e);
    }
    println!("Login successful, proceeding with scraping");

    let counter = Arc::new(AtomicUsize::new(0));
    let client = reqwest::Client::new();

    let mut stdout = stdout();
    if let Err(e) = execute!(stdout, EnterAlternateScreen, Hide) {
        eprintln!("Failed to set up alternate screen: {:?}", e);
        driver.quit().await?;
        return Err(e.into());
    }

    let counter_clone = Arc::clone(&counter);
    let max_images = args.max_images;
    
    println!("Starting progress display...");
    tokio::spawn(async move {
        display_scrape_progress(counter_clone, max_images).await;
    });

    println!("Beginning scraping process...");
    if let Some(csv_file) = args.csv_file.as_ref() {
        println!("Processing CSV file: {}", csv_file);
        if let Err(e) = process_csv_file(&driver, &client, csv_file, &args, &counter).await {
            eprintln!("Error processing CSV: {:?}", e);
        } else {
            println!("CSV processing completed successfully.");
        }
    } else if let Some(url) = args.url.as_ref() {
        println!("Processing single URL: {}", url);
        if let Err(e) = process_single_url(&driver, &client, url, &args, &counter).await {
            eprintln!("Error processing URL: {:?}", e);
        } else {
            println!("URL processing completed successfully.");
        }
    }

    println!("Cleaning up...");
    if let Err(e) = driver.quit().await {
        eprintln!("Error quitting WebDriver: {:?}", e);
    }
    if let Err(e) = execute!(stdout, Show, LeaveAlternateScreen) {
        eprintln!("Error restoring terminal: {:?}", e);
    }

    println!("Scraping process completed.");
    Ok(())
}