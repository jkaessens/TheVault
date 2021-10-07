table! {
    fastq (sample_id, filename) {
        filename -> Varchar,
        sample_id -> Int4,
    }
}

table! {
    run (name) {
        name -> Varchar,
        date -> Date,
        assay -> Varchar,
        chemistry -> Varchar,
        description -> Nullable<Varchar>,
        investigator -> Varchar,
        path -> Text,
    }
}

table! {
    sample (id) {
        run -> Varchar,
        name -> Varchar,
        dna_nr -> Varchar,
        project -> Varchar,
        lims_id -> Nullable<Int8>,
        primer_set -> Nullable<Varchar>,
        id -> Int4,
        cells -> Nullable<Int4>,
    }
}

table! {
    samplesheet (id) {
        id -> Int4,
        created -> Timestamp,
        basket -> Text,
    }
}

joinable!(fastq -> sample (sample_id));
joinable!(sample -> run (run));

allow_tables_to_appear_in_same_query!(
    fastq,
    run,
    sample,
    samplesheet,
);
