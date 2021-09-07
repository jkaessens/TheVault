-- This file was automatically created by Diesel to setup helper functions
-- and other internal bookkeeping. This file is safe to edit, any future
-- changes will be added to existing projects as new migrations.




-- Sets up a trigger for the given table to automatically set a column called
-- `updated_at` whenever the row is modified (unless `updated_at` was included
-- in the modified columns)
--
-- # Example
--
-- ```sql
-- CREATE TABLE users (id SERIAL PRIMARY KEY, updated_at TIMESTAMP NOT NULL DEFAULT NOW());
--
-- SELECT diesel_manage_updated_at('users');
-- ```
CREATE OR REPLACE FUNCTION diesel_manage_updated_at(_tbl regclass) RETURNS VOID AS $$
BEGIN
    EXECUTE format('CREATE TRIGGER set_updated_at BEFORE UPDATE ON %s
                    FOR EACH ROW EXECUTE PROCEDURE diesel_set_updated_at()', _tbl);
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION diesel_set_updated_at() RETURNS trigger AS $$
BEGIN
    IF (
        NEW IS DISTINCT FROM OLD AND
        NEW.updated_at IS NOT DISTINCT FROM OLD.updated_at
    ) THEN
        NEW.updated_at := current_timestamp;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TABLE public.fastq (
    filename character varying(1024) NOT NULL,
    sample_id integer NOT NULL
);

CREATE TABLE public.run (
    name character varying(100) NOT NULL,
    date date NOT NULL,
    assay character varying NOT NULL,
    chemistry character varying NOT NULL,
    description character varying,
    investigator character varying(8) NOT NULL,
    path text NOT NULL
);

CREATE TABLE public.sample (
    run character varying(200) NOT NULL,
    name character varying(200) NOT NULL,
    dna_nr character varying(200) NOT NULL,
    project character varying(200) NOT NULL,
    lims_id bigint,
    primer_set character varying(200),
    id serial NOT NULL,
    cells integer
);


ALTER TABLE ONLY public.fastq
    ADD CONSTRAINT fastq_pkey PRIMARY KEY (sample_id, filename);

ALTER TABLE ONLY public.run
    ADD CONSTRAINT run_pkey PRIMARY KEY (name);

ALTER TABLE ONLY public.sample
    ADD CONSTRAINT sample_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.fastq
    ADD CONSTRAINT fastq_fkey_sample FOREIGN KEY (sample_id) REFERENCES public.sample(id) ON UPDATE CASCADE ON DELETE CASCADE;

ALTER TABLE ONLY public.sample
    ADD CONSTRAINT sample_fkey_run FOREIGN KEY (run) REFERENCES public.run(name) ON UPDATE CASCADE ON DELETE CASCADE NOT VALID;

CREATE INDEX fastq_filename_idx ON fastq USING btree (filename ASC NULLS LAST);
CREATE INDEX fastq_sampleid_idx ON fastq USING btree (sample_id ASC NULLS LAST);

