// examples/test_get_tree.rs
use reqwest::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_url = std::env::var("BASE_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    let client = Client::new();
    let user_id = "shpirkov@gmail.com";

    let url = format!("{}/api/agent/tree/{}", base_url, user_id);

    println!("Fetching tree for user: {}", user_id);
    println!("URL: {}\n", url);

    match client.get(&url).send().await {
        Ok(response) => {
            println!("Status: {}", response.status());
            let body = response.text().await?;
            println!("Response:\n{}", body);
        }
        Err(e) => eprintln!("Error: {}", e),
    }

    Ok(())
}
