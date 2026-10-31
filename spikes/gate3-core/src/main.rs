use gate3_core::{CoreSession, NativeEnv, addon_lines, first_movie_catalog};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(message) = run().await {
        eprintln!("gate3-core: {message}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), &'static str> {
    let mut catalog = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--catalog" => catalog = true,
            _ => return Err("usage: gate3-core [--catalog]"),
        }
    }
    let api_base = std::env::var("GATE3_API_BASE")
        .ok()
        .map(|base| base.parse())
        .transpose()
        .map_err(|_| "invalid API override")?;
    NativeEnv::initialize(api_base)?;
    let mut session = CoreSession::new().await?;
    let mut profile = session.profile()?;
    println!("signed out: addon count={}", profile.addons.len());
    for line in addon_lines(&profile.addons) {
        println!("{line}");
    }
    match (
        std::env::var("PANORAMA_STREMIO_EMAIL"),
        std::env::var("PANORAMA_STREMIO_PASSWORD"),
    ) {
        (Ok(email), Ok(password)) => {
            profile = session.sign_in(email, password).await?;
            let domain = profile
                .auth
                .as_ref()
                .and_then(|auth| auth.user.email.rsplit_once('@').map(|(_, domain)| domain))
                .unwrap_or("(unknown)");
            println!(
                "signed in: email domain={domain} addon count={}",
                profile.addons.len()
            );
            for line in addon_lines(&profile.addons) {
                println!("{line}");
            }
        }
        (Err(_), Err(_)) => println!("real sign-in: not run, no credentials"),
        _ => return Err("set both PANORAMA_STREMIO_EMAIL and PANORAMA_STREMIO_PASSWORD"),
    }
    if catalog {
        println!("first movie catalog: first 5 item names");
        for name in first_movie_catalog(&profile.addons).await? {
            println!("item name={name:?}");
        }
    }
    Ok(())
}
