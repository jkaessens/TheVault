
use diesel::QueryDsl;
use diesel::RunQueryDsl;
use rocket::fs::FileServer;
use diesel::ExpressionMethods;
use rocket_dyn_templates::Template;
use rocket::fs::relative;
use rocket::form::FromForm;

use crate::models::*;
use crate::schema::*;


use crate::vaultdb::VaultDatabase;
use std::collections::HashMap;

macro_rules! context {
    ($($key:ident $(: $value:expr)?),*$(,)?) => {{
        use serde::ser::{Serialize, Serializer, SerializeMap};
        use ::std::fmt::{Debug, Formatter};

        #[allow(non_camel_case_types)]
        struct ContextMacroCtxObject<$($key: Serialize),*> {
            $($key: $key),*
        }

        #[allow(non_camel_case_types)]
        impl<$($key: Serialize),*> Serialize for ContextMacroCtxObject<$($key),*> {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where S: Serializer,
            {
                let mut map = serializer.serialize_map(None)?;
                $(map.serialize_entry(stringify!($key), &self.$key)?;)*
                map.end()
            }
        }

        #[allow(non_camel_case_types)]
        impl<$($key: Debug + Serialize),*> Debug for ContextMacroCtxObject<$($key),*> {
            fn fmt(&self, f: &mut Formatter<'_>) -> ::std::fmt::Result {
                f.debug_struct("context!")
                    $(.field(stringify!($key), &self.$key))*
                    .finish()
            }
        }

        ContextMacroCtxObject {
            $($key $(: $value)?),*
        }
    }};
}

#[derive(FromForm, Debug)]
struct Query {
    filters: Option<String>,
    limit: Option<usize>,
}

#[get("/run/<runid>")]
async fn get_run(conn: VaultDatabase, runid: String) -> Template {
    let runs: Vec<Run> = conn.run(|c| run::table.filter(run::name.eq(runid)).load(c).expect("Error loading run")).await;

    Template::render("run", context!{
        runs
    })
}

/*

#[get("/")]
async fn index() -> Template {
    Template::render("query", context!{ runs: Vec::<Run>::new()} )
}
*/
#[get("/?<filter>&<limit>")]
async fn run_query(conn: VaultDatabase, filter: Option<String>, limit: Option<usize>) -> Template {
    
    let mut filters: HashMap<String, String> = HashMap::new();
    let mut warnings: Vec<String> = Vec::new();

    debug!("filter: {:?} limit: {:?}", filter, limit);

    if let Some(filter_str) = filter.as_ref() {
        for f in filter_str.split_whitespace() {
            let parts: Vec<&str> = f.split("=").collect();
            if parts.len() != 2 {
                warnings.push(format!("Ignoring filter '{}' which must be in the form KEY=VALUE.", &f));
            } else {
                filters.insert(parts[0].to_string(), parts[1].to_string());
            }
        }
    }

    let mut samples: Vec<Sample> = if filter.is_some() || limit.is_some() {
        conn.run(move |c| {
            crate::vaultdb::query(c, "%.fastq.gz", &filters, limit.clone()).into_keys().collect::<Vec<Sample>>()
        }).await
    } else {
        Vec::new()
    };
    
    samples.sort_unstable();
    let count = samples.len();

    Template::render("query", context!{
        filters: filter, 
        limit,
        warnings,
        samples,
        count,
    })
}

#[rocket::main]
pub async fn rocket ()  {
    let figment = rocket::Config::figment();
    if let Err(e) = rocket::custom(figment)
        .attach(VaultDatabase::fairing())
        .attach(Template::fairing())
        .mount("/static", FileServer::from(relative!("static")))
        .mount("/", routes![run_query, get_run])
        .launch()
        .await {
            error!("Could not launch rocket: {}", e);
    }
}
