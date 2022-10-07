use aws_config::meta::region::RegionProviderChain;
use aws_sdk_imagebuilder::model::{Filter, ImagePipeline, ImageRecipe, ImageRecipeSummary};
use aws_sdk_imagebuilder::{Client, Error};
use clap::Parser;
use semver::Version;

#[derive(Parser)]
struct Cli {
    #[arg(required = true)]
    name: Option<String>,

    #[arg(short, long, value_name = "AMI_ID", required = true)]
    ami_id: Option<String>,
}

fn version(s: &ImageRecipeSummary) -> Version {
    Version::parse(s.arn.as_ref().unwrap().split("/").last().unwrap()).unwrap()
}

async fn get_image_recipe(client: &Client, filter: &Filter) -> Result<ImageRecipe, Error> {
    let mut latest: Option<ImageRecipeSummary> = None;
    let mut next_token: Option<String> = None;

    loop {
        let r = client
            .list_image_recipes()
            .filters(filter.clone())
            .set_next_token(next_token)
            .send()
            .await?;

        let mut l = r.image_recipe_summary_list.unwrap();

        l.sort_by_key(|k| version(k));

        let new = l.last().unwrap().clone();

        match latest {
            Some(ref old) => {
                if version(&new) > version(&old) {
                    latest = Some(new);
                }
            }
            _ => latest = Some(new),
        };

        next_token = r.next_token;
        if next_token == None {
            break;
        }
    }

    let r = client
        .get_image_recipe()
        .set_image_recipe_arn(latest.unwrap().arn)
        .send()
        .await?;

    Ok(r.image_recipe.unwrap())
}

async fn get_image_pipeline(client: &Client, filter: &Filter) -> Result<ImagePipeline, Error> {
    Ok(client
        .list_image_pipelines()
        .filters(filter.clone())
        .send()
        .await?
        .image_pipeline_list
        .unwrap()
        .first()
        .unwrap()
        .clone())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");

    let config = aws_config::from_env().region(region_provider).load().await;

    let client = Client::new(&config);

    let cli = Cli::parse();

    let filter = Filter::builder()
        .name("name")
        .values(cli.name.unwrap())
        .build();

    let image_recipe = get_image_recipe(&client, &filter).await?;

    if image_recipe.parent_image != cli.ami_id {
        let mut new_version = Version::parse(&image_recipe.version.unwrap()).unwrap();
        new_version.patch += 1;

        let r = client
            .create_image_recipe()
            .semantic_version(new_version.to_string())
            .set_block_device_mappings(image_recipe.block_device_mappings)
            .set_components(image_recipe.components)
            .set_name(image_recipe.name)
            .set_parent_image(cli.ami_id)
            .send()
            .await?;

        let image_pipeline = get_image_pipeline(&client, &filter).await?;

        let r = client
            .update_image_pipeline()
            .set_image_pipeline_arn(image_pipeline.arn)
            .set_image_recipe_arn(r.image_recipe_arn)
            .set_infrastructure_configuration_arn(image_pipeline.infrastructure_configuration_arn)
            .send()
            .await?;

        println!("{:?}", r);
    }

    Ok(())
}
