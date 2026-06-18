use std::io::Cursor;
use std::time::Instant;

use base64::{Engine, prelude::BASE64_STANDARD};
use cx58_agent::helper::client;
use image::{GenericImageView, ImageFormat};
use rig::message::{DocumentSourceKind, Message, UserContent};
use rig::prelude::*;
use rig::{
    OneOrMany,
    completion::{Prompt, message::Image},
    message::ImageMediaType,
};
use tokio::fs;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let is_local = true;
    //let model = REMOTE_MODELS[10];
    let model = "qwen3-vl";
    //let img: &str = "D:/projects/rust/cx/cx58-agent/data/3w_5.jpg";
    //let img: &str = "./data/4к_1.jpg";
    let img: &str = "./data/4k_4.jpg";
    //let prompt = "In this picture, all I see only three empty window openings.";
    let prompt = "Describe the picture!";
    describe(model, is_local, img, prompt).await?;
    Ok(())
}

async fn describe(
    model: &str,
    is_local: bool,
    img: &str,
    prompt: &str,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    // Tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let json = serde_json::json!({
        "format": "json"
    });
    let client = client(false);

    //let language = "English";
    //let language = "German";
    // Translate responce to: {}
    // Translate responce to German.

    //let system = create_description_system_prompt();
    let system = create_description_system_big_prompt();
    //let system = create_description_prompt_with_schema();

    // Create agent with a single context prompt
    let agent = client
        .agent(model)
        // .agent("llama3.2-vision")
        .additional_params(json)
        .preamble(&system)
        .temperature(0.1)
        .build();

    // Read image and convert to base64
    let image_bytes = fs::read(img).await?;
    let scaled = resize_image_to_bytes(&image_bytes, 1200, 1200)?;
    let image_base64 = BASE64_STANDARD.encode(scaled);

    // Compose `Image` for prompt
    let image = Image {
        data: DocumentSourceKind::base64(&image_base64),
        media_type: Some(ImageMediaType::JPEG),
        ..Default::default()
    };
    let response = agent
        .prompt(Message::User {
            content: OneOrMany::many(vec![
                UserContent::Text(prompt.into()),
                UserContent::Image(image),
            ])?,
        })
        .await?;

    // Prompt the agent and print the response
    //let response = agent.prompt(image).await?;

    println!("{response}");
    println!("Time elapsed: {:?}", start.elapsed());

    Ok(())
}

fn resize_image_to_bytes(
    image_bytes: &[u8],
    output_width: u32,
    output_height: u32,
) -> Result<Vec<u8>, image::ImageError> {
    // 1. Open the image file
    let img = image::load_from_memory(image_bytes)?;
    //println!("Original dimensions: {:?}", img.dimensions());
    if img.height() <= output_height && img.width() <= output_width {
        return Ok(image_bytes.to_vec());
    }
    // 2. Resize the image (using the Lanczos3 filter for high quality)
    let resized_img = img.resize(
        output_width,
        output_height,
        image::imageops::FilterType::Lanczos3,
    );
    println!("Resized dimensions: {:?}", resized_img.dimensions());

    // 3. Encode the resized image into a byte array (Vec<u8>) in memory
    let mut bytes: Vec<u8> = Vec::new();

    // We use a std::io::Cursor to allow the encoder to write to our Vec<u8> as if it were a file
    let mut cursor = Cursor::new(&mut bytes);

    // Encode as JPEG. You can change this to PNG, GIF, etc., as needed
    resized_img.write_to(&mut cursor, ImageFormat::Jpeg)?;
    // The 'bytes' vector now contains the image data
    Ok(bytes)
}
/*
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Description {
    description: String,
    windows: String,
    doors: String,
    radiators: String,
}
*/

pub fn create_description_system_prompt() -> String {
    format!(
        r#"
You are a precise, reliable, and concise assistant.
You are an expert in construction description.
Your specialization is only windows, doors, radiators and empty openings for future installation of windows and doors.
If any windows, doors, or radiators are missing and there are only bare openings, be sure to describe this in detail!
It is necessary to describe in detail the quantity, material, condition, completeness and stage of installation of windows, doors and radiators.
An error in determining presence or quantity is very bad!
Don't let me down with the definitions and calculations.
Don't show empty descriptions!
This is a photo of a construction site, so you might see exposed concrete or brick.
If so, please describe it.
Don't invent what you don't see!

Response format (JSON only, no other text):
{{
  "description": "General and complete description of the object",
  "windows": "Detailed information about windows only",
  "doors": "Detailed information about doors only",
  "radiators": "Detailed information about radiators only",
  "openings": "Detailed information about openings only"
}}
"#
    )
}
pub fn create_description_system_big_prompt() -> String {
    format!(
        r#"
You are a precise, reliable, and concise assistant.
You are an expert in construction description.
Your specialization is only windows, doors, radiators and empty openings for future installation of windows and doors.
If any windows, doors, or radiators are missing and there are only bare openings, be sure to describe this in detail!
It is necessary to describe in detail the quantity, material, condition, completeness and stage of installation of windows, doors and radiators.
An error in determining presence or quantity is very bad!
Don't let me down with the definitions and calculations.
Don't show empty descriptions!
This is a photo of a construction site, so you might see exposed concrete or brick.
If so, please describe it.
Don't invent what you don't see!

Response format (JSON only, no other text):
{{
  "description": "General and complete description of the object",
  "windows": "Detailed information about windows only",
  "doors": "Detailed information about doors only",
  "radiators": "Detailed information about radiators only",
  "openings": "Detailed information about openings only"
}}

EXAMPLE OUTPUT:
{{
  "description": "A modern residential room with white walls and wooden flooring",
  "windows": "Two large double-glazed windows on the eastern wall with white frames",
  "doors": "Single wooden door with silver handle on the northern wall",
  "radiators": "White panel radiator mounted beneath the window",
  "openings": null
}}

CRITICAL INSTRUCTIONS:
- Return ONLY the JSON object, no additional text
- Do not wrap the JSON in markdown code blocks
- Use null for fields where elements are not visible
- Ensure all strings are properly escaped
- The "description" field must always be present and non-empty
"#
    )
}

/// Alternative: Create prompt with schema example
pub fn create_description_prompt_with_schema() -> String {
    r#"You are an expert at analyzing architectural and construction images.
Analyze the provided image and generate a detailed description in JSON format.

REQUIRED OUTPUT FORMAT:
Return ONLY a valid JSON object matching this schema:

{
  "description": "string (REQUIRED) - General and complete description of the object",
  "windows": "string or null - Detailed information about windows only",
  "doors": "string or null - Detailed information about doors only",
  "radiators": "string or null - Detailed information about radiators only",
  "openings": "string or null - Detailed information about openings only"
}

EXAMPLE OUTPUT:
{
  "description": "A modern residential room with white walls and wooden flooring",
  "windows": "Two large double-glazed windows on the eastern wall with white frames",
  "doors": "Single wooden door with silver handle on the northern wall",
  "radiators": "White panel radiator mounted beneath the window",
  "openings": null
}

CRITICAL INSTRUCTIONS:
- Return ONLY the JSON object, no additional text
- Do not wrap the JSON in markdown code blocks
- Use null for fields where elements are not visible
- Ensure all strings are properly escaped
- The "description" field must always be present and non-empty"#
        .to_string()
}
