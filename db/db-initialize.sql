CREATE TABLE public.fastq (
    filename character varying(1024) NOT NULL,
    sample_id integer NOT NULL,
    primer_set character varying,
    lane integer NOT NULL,
    r integer NOT NULL
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
    id serial NOT NULL
);


ALTER TABLE ONLY public.fastq
    ADD CONSTRAINT fastq_pkey PRIMARY KEY (filename);

ALTER TABLE ONLY public.fastq
    ADD CONSTRAINT fastq_uniq UNIQUE (sample_id, primer_set, lane, r);

ALTER TABLE ONLY public.run
    ADD CONSTRAINT run_pkey PRIMARY KEY (name);

ALTER TABLE ONLY public.sample
    ADD CONSTRAINT sample_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.sample
    ADD CONSTRAINT sample_unique UNIQUE (run, name, dna_nr);

ALTER TABLE ONLY public.fastq
    ADD CONSTRAINT fastq_fkey_sample FOREIGN KEY (sample_id) REFERENCES public.sample(id) ON UPDATE CASCADE ON DELETE CASCADE;

ALTER TABLE ONLY public.sample
    ADD CONSTRAINT sample_fkey_run FOREIGN KEY (run) REFERENCES public.run(name) ON UPDATE CASCADE ON DELETE CASCADE NOT VALID;


