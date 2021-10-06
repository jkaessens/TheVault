-- Your SQL goes here
CREATE TABLE samplesheet (
    id serial primary key,
    created timestamp not null default now (),
    basket text not null
);
