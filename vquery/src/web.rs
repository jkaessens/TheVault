
use diesel::QueryDsl;
use diesel::RunQueryDsl;
use rocket::fs::FileServer;
use diesel::ExpressionMethods;
use rocket_dyn_templates::Template;
use rocket::fs::relative;

use crate::models::*;
use crate::schema::*;


use crate::vaultdb::VaultDatabase;
use std::collections::HashMap;


#[get("/run/<runid>")]
async fn get_run(conn: VaultDatabase, runid: String) -> Template {
    let run = conn.run(|c| run::table.filter(run::name.eq(runid)).load(c).expect("Error loading run")).await;

    let mut c: HashMap<&str, Vec<Run>> = HashMap::new();
    c.insert("runs", run);
    Template::render("run", &c)
}

#[get("/")]
async fn index(conn: VaultDatabase) -> Template {
    let runs: Vec<Run> = conn.run(|c: &mut diesel::PgConnection| {
        run::table.limit(5).load::<Run>(c).expect("Error loading runs")
    }).await;

    let mut c : HashMap<&str,Vec<Run>> = HashMap::new();
    c.insert("runs", runs);
    

    Template::render("runs", &c)
}


#[rocket::main]
pub async fn rocket ()  {
    let figment = rocket::Config::figment();
    if let Err(e) = rocket::custom(figment)
        .attach(VaultDatabase::fairing())
        .attach(Template::fairing())
        .mount("/static", FileServer::from(relative!("static")))
        .mount("/", routes![index, get_run])
        .launch()
        .await {
            error!("Could not launch rocket: {}", e);
    }
}
