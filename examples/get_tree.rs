use reqwest::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_url = std::env::var("BASE_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    let client = Client::new();
    let user_id = "shpirkov@gmail.com";

    // Test 1: без листьев (with_leafs = false)
    let url = format!("{}/api/agent/tree/{}", base_url, user_id);
    println!("Test 1: without leafs");
    println!("URL: {}\n", url);

    match client.get(&url).send().await {
        Ok(response) => {
            println!("Status: {}", response.status());
            let body = response.text().await?;
            println!("Response:\n{}\n", body);
        }
        Err(e) => eprintln!("Error: {}\n", e),
    }

    // Test 2: с листьями (with_leafs = true)
    let url_with_leafs = format!("{}?with_leafs=true", url);
    println!("Test 2: with leafs");
    println!("URL: {}\n", url_with_leafs);

    match client.get(&url_with_leafs).send().await {
        Ok(response) => {
            println!("Status: {}", response.status());
            let body = response.text().await?;
            println!("Response:\n{}", body);
        }
        Err(e) => eprintln!("Error: {}", e),
    }

    Ok(())
}
