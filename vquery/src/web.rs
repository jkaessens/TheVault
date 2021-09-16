
use diesel::QueryDsl;
use diesel::RunQueryDsl;
use rocket::fs::FileServer;
use diesel::ExpressionMethods;
use rocket_dyn_templates::Template;
use rocket::fs::relative;
use rocket::form::FromForm;
use rocket_dyn_templates::handlebars::Handlebars;
use rocket_dyn_templates::handlebars::no_escape;

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

    if let Some(filter_str) = filter.as_ref() {
        for f in filter_str.split_whitespace() {
            let parts: Vec<&str> = f.split("=").collect();
            match parts.len() {
                1 => {
                    warnings.push(format!("Invalid filter <span class=\"font-monospace\">{}</span> rewritten as <span class=\"font-monospace\">filename=%{}%</span>. Please consult the syntax help.", parts[0], parts[0]));
                    filters.insert(String::from("filename"), format!("%{}%", parts[0]));
                }
                2 => {
                    if !["run","name","dna_nr","project","primer_set","filename","cells","cells<","cells>","lims_id","lims_id<","lims_id>"].contains(&parts[0]) {
                        warnings.push(format!("Ignoring unknown filter column <span class=\"font-monospace\">{}</span>", parts[0]));
                    } else {
                        if parts[0] == "dna_nr" {
                            let norm_dna_nr = parts[1].replace("D-", "");
                            filters.insert(parts[0].to_string(), norm_dna_nr);
                        } else {
                            filters.insert(parts[0].to_string(), parts[1].to_string());
                        }
                    }
                }
                _ => {
                    warnings.push(String::from("Invalid filter string. Only zero or more <span class=\"font-monospace\">key=value</span> pairs are allowed. Please consult the syntax help."));
                }
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

pub fn customize_hbs(hbs: &mut Handlebars) {
    hbs.register_escape_fn(no_escape);
}

#[rocket::main]
pub async fn rocket ()  {
    let figment = rocket::Config::figment();
    if let Err(e) = rocket::custom(figment)
        .attach(VaultDatabase::fairing())
        .attach(Template::custom(|engines| { customize_hbs(&mut engines.handlebars)} ))
        .mount("/static", FileServer::from(relative!("static")))
        .mount("/", routes![run_query, get_run])
        .launch()
        .await {
            error!("Could not launch rocket: {}", e);
    }
}
