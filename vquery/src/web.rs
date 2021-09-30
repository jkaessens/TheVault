
use rocket::form::Form;
use rocket::fs::FileServer;
use rocket::fs::TempFile;
use rocket::http::Cookie;
use rocket::http::CookieJar;
use rocket_dyn_templates::Template;
use rocket::fs::relative;
use rocket::form::FromForm;
use rocket_dyn_templates::handlebars::Handlebars;
use rocket_dyn_templates::handlebars::no_escape;
use diesel::QueryDsl;
use diesel::RunQueryDsl;
use diesel::ExpressionMethods;

use crate::models::*;

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

fn parse_filters(filter_str: &str, warnings: &mut Vec<String>) -> HashMap<String,String> {
    let mut filters = HashMap::new();
    for f in filter_str.split_whitespace() {
        let parts: Vec<&str> = f.split('=').collect();
        match parts.len() {
            1 => {
                warnings.push(format!("Invalid filter <span class=\"font-monospace\">{}</span> rewritten as <span class=\"font-monospace\">filename=%{}%</span>. Please consult the syntax help.", parts[0], parts[0]));
                filters.insert(String::from("filename"), format!("%{}%", parts[0]));
            }
            2 => {
                if !["run","name","dna_nr","project","primer_set","filename","cells","cells<","cells>","lims_id","lims_id<","lims_id>"].contains(&parts[0]) {
                    warnings.push(format!("Ignoring unknown filter column <span class=\"font-monospace\">{}</span>", parts[0]));
                } else if parts[0] == "dna_nr" {
                    let norm_dna_nr = parts[1].replace("D-", "");
                    filters.insert(parts[0].to_string(), norm_dna_nr);
                } else {
                    filters.insert(parts[0].to_string(), parts[1].to_string());
                }
            }
            _ => {
                warnings.push(String::from("Invalid filter string. Only zero or more <span class=\"font-monospace\">key=value</span> pairs are allowed. Please consult the syntax help."));
            }
        };
    }
    filters
}

#[derive(FromForm, Debug)]
struct QueryResult<'a> {
    #[field(name="filter")]
    filters: Option<&'a str>,

    limit: Option<usize>,

    #[field(name="sample")]
    selected_samples: Option<HashMap<&'a str, bool>>,

    #[field(name="import_ssheet")]
    samplesheet: Option<TempFile<'a>>,

    #[field(name="import_cols")]
    samplesheet_cols: Option<&'a str>,

    #[field(name="basket_id")]
    samplesheet_id: Option<i32>,
}


#[route(POST, uri = "/checkout", data = "<cart>")]
async fn checkout(conn: VaultDatabase, cart: Form<QueryResult<'_>>, cookies: &CookieJar<'_>) -> Template {

    let mut selected_samples: Vec<i32> = Vec::new();
    if let Some(ss) = &cart.selected_samples {
        selected_samples = ss.keys().filter_map(|k| k.parse::<i32>().ok()).collect();
    }
    if let Some(c) = cookies.get("selected_samples") {
        selected_samples.append(&mut c.value().split(',').filter_map(|k| k.parse::<i32>().ok()).collect::<Vec<i32>>());
    }
    selected_samples.sort_unstable();
    selected_samples.dedup();

    // add any samples that have been received via FormRequest to the cookie
    let cookie_val = selected_samples.iter().map(|i| i.to_string()).collect::<Vec<String>>().join(",");
    cookies.add(Cookie::new("selected_samples", cookie_val));

    debug!("Cart: {:?}", &cart);

    use crate::schema::sample;
    let samples: Vec<Sample> = conn.run(|c| sample::table.filter(sample::id.eq_any(selected_samples)).load(c).expect("Error loading samples")).await;
    //let mut samples = samples.into_iter().map(|ss| ss.to_model()).collect::<Vec<crate::sample::Sample>>();
    
    let _cols = cart.samplesheet_cols.unwrap_or_default();
    // load samplesheet, if we have one
    let samplesheet_id = 0;
    // let samplesheet_id = if let Some(ss) = &mut cart.samplesheet {
    //     let cols: Vec<&str> = cols.split(",").collect();
    //     //load_samplesheet(ss, samples.as_slice(), cols.as_slice())
    // } else if let Some(i) = cart.samplesheet_id {
    //     i
    // } else {
    //     0
    // };

    //update_samples(&mut conn, samplesheet_id, &mut samples);

    Template::render("checkout", context!{
        samples,
        samplesheet_id
    })
}

#[post("/", data = "<query>")]
async fn run_query(conn: VaultDatabase, cookies: &CookieJar<'_>, query: Form<QueryResult<'_>>) -> Template {
    let mut filters: HashMap<String, String> = HashMap::new();
    let mut warnings: Vec<String> = Vec::new();
    let query = query.into_inner();

    debug!("POST /: query {:?}", &query);

    if let Some(filter_str) = query.filters.as_ref() {
        filters = parse_filters(filter_str, &mut warnings);
    }

    let mut samples: Vec<Sample> = if query.filters.is_some() || query.limit.is_some() {
        let limit = query.limit;
        conn.run(move |c| {
            crate::vaultdb::query(c, "%.fastq.gz", &filters, limit).into_keys().collect::<Vec<Sample>>()
        }).await
    } else {
        Vec::new()
    };

    let mut selected_samples: Vec<&str> = Vec::new();
    if let Some(ss) = query.selected_samples {
        selected_samples.append(&mut ss.into_keys().collect::<Vec<&str>>());
    }

    // If there is a cookie, also pull the selected samples from there.
    if let Some(ss) = cookies.get("selected_samples") {
        selected_samples.append(&mut ss.value().split(',').collect::<Vec<&str>>());
    }
    selected_samples.sort_unstable();
    selected_samples.dedup();

    // add any samples that have been received via FormRequest to the cookie
    let cookie_val = selected_samples.join(",");
    cookies.add(Cookie::new("selected_samples", cookie_val));
    
    samples.sort_unstable();
    let count = samples.len();
    let selected_samples = samples.iter().map(|s| if selected_samples.contains(&s.id.to_string().as_ref()) { 1 } else { 0 } ).collect::<Vec<u8>>();
    
    Template::render("query", context!{
        filters: query.filters, 
        limit: query.limit,
        warnings,
        samples,
        count,
        selected_samples,
    })
}

#[get("/?<filter>&<limit>")]
async fn run_query_default(conn: VaultDatabase, filter: Option<String>, limit: Option<usize>, cookies: &CookieJar<'_>) -> Template {
    
    let mut filters: HashMap<String, String> = HashMap::new();
    let mut warnings: Vec<String> = Vec::new();

    if let Some(filter_str) = filter.as_ref() {
        filters = parse_filters(filter_str, &mut warnings);
    }

    let mut samples: Vec<Sample> = if filter.is_some() || limit.is_some() {
        conn.run(move |c| {
            crate::vaultdb::query(c, "%.fastq.gz", &filters, limit).into_keys().collect::<Vec<Sample>>()
        }).await
    } else {
        Vec::new()
    };
    
    samples.sort_unstable();
    let count = samples.len();

    cookies.remove(Cookie::named("selected_samples"));

    Template::render("query", context!{
        filters: filter, 
        limit,
        warnings,
        samples,
        count,
        selected_samples: Vec::<u8>::new()
    })
}

pub fn customize_hbs(hbs: &mut Handlebars) {
    hbs.register_escape_fn(no_escape);
    hbs.set_strict_mode(true);
}

#[rocket::main]
pub async fn rocket ()  {
    let figment = rocket::Config::figment();
    if let Err(e) = rocket::custom(figment)
        .attach(VaultDatabase::fairing())
        .attach(Template::custom(|engines| { customize_hbs(&mut engines.handlebars)} ))
        .mount("/static", FileServer::from(relative!("static")))
        .mount("/", routes![run_query, run_query_default, checkout])
        .launch()
        .await {
            error!("Could not launch rocket: {}", e);
    }
}
