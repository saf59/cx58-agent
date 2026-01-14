mod common;
use reqwest::{Client, Response};
use cx58_agent::db::{NodeType, TreeNode};
use crate::common::response_tree::{build_tree, NodeData, Tree};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_url = std::env::var("BASE_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    let client = Client::new();
    let user_id = "shpirkov@gmail.com";

    // with_leafs = false
    let url = format!("{}/agent/tree/{}", base_url, user_id);
    println!("Test 1: without leafs");
    println!("URL: {}\n", url);

    match client.get(&url).send().await {
        Ok(response) => {
            print_response(response).await;
        }
        Err(e) => eprintln!("Error: {}\n", e),
    }

    // with_leafs = true
    let url_with_leafs = format!("{}?with_leafs=true", url);
    println!("Test 2: with leafs");
    println!("URL: {}\n", url_with_leafs);

    match client.get(&url_with_leafs).send().await {
        Ok(response) => {
            print_response(response).await;
        }
        Err(e) => eprintln!("Error: {}", e),

    }

    Ok(())
}

async fn print_response(response: Response) {
    if response.status().is_success() {
        // Try to parse response as Vec<TreeNode>
        match response.json::<Vec<TreeNode>>().await {
            Ok(nodes) => {
                println!("\n=== Tree Structure ===");
                let tree = build_tree(nodes);
                print_tree(&tree, 0);
                println!("======================\n");
            }
            Err(e) => {
                eprintln!("  ⚠ Could not parse response as tree: {}", e);
            }
        }
    } else {
        eprintln!("  ✗ get tree failed: {}", response.status());
    }
}

/// Pretty print the tree structure with indentation
fn print_tree(nodes: &[Tree], indent: usize) {
    let prefix = "  ".repeat(indent);

    for node in nodes {
        let node_type_symbol = match node.node_type {
            NodeType::Root => "🌳",
            NodeType::Branch => "📁",
            NodeType::ImageLeaf => "🖼️",
        };

        let name = node.name.as_deref().unwrap_or("(unnamed)");

        print!("{}{} {} [{}]", prefix, node_type_symbol, name, node.node_type_str());

        // Print typed data info
        match &node.data {
            NodeData::Image(img) => {
                if let Some(src) = &img.src {
                    println!("\n{}   SRC: {}", prefix, src);
                }
                if let Some(url) = &img.url {
                    println!("{}   URL: {}", prefix, url);
                }
                if let Some(size) = img.size {
                    println!("{}   Size: {} bytes", prefix, size);
                }
            }
            NodeData::Branch(branch) => {
                if let Some(title) = &branch.title {
                    println!("{}   Title: {}", prefix, title);
                }
            }
            NodeData::Empty => {}
        }

        println!("{}   Own: {} | Depth: {} | Updated: {}",
                 prefix, node.own, node.depth, node.updated_at.format("%Y-%m-%d %H:%M"));

        // Recursively print children
        if !node.children.is_empty() {
            print_tree(&node.children, indent + 1);
        }
    }
}

