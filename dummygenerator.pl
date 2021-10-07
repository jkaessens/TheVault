#!/usr/bin/env perl

use strict;
use warnings;

my @primersets = qw(IGHFR1 IGHDJ TRBDb IGK TRD);
my @names = qw(Karol Jan Nikos Tomas Jakub Tesla Einstein Newton Röntgen Hawking Heisenberg Lorentz Becquerel Skłodowska-Curie);
my @projects = ("Top Secret", "Vault 42", "World Dominance");

my $undef_rate = 33; # percent

sub usage {
    print "Syntax: $0 <runs> <samples per run> <fastqs per samle>\n";
    print "Will dump SQL commands on stdout.\n";
    exit 1;
}
sub random_string {
    my $length = shift;
    my $string;
    my @chars = ('A'..'Z');
    while($length--) {
        $string .= $chars[rand @chars];
    }
    return $string;
}

sub random_value {
    my $length = shift;
    my $string;
    my @chars = ('0'..'9');
    while($length--) {
        $string .= $chars[rand @chars];
    }
    return $string;
}

sub print_sometimes {
    my $val = shift;
    my $number = shift;
    if(rand(100) > $undef_rate) {
        if($number) {
            print "$val";

        } else {
            print "'$val'";
        }
    } else {
        print "NULL";
    }
}

my $num_runs = shift @ARGV or usage;
my $num_samples = shift @ARGV or usage;
my $num_fastqs = shift @ARGV or usage;

my $next_id = 1;
for (1..$num_runs) {
    # make up run name
    # 210404_M12345_0000_000000000-ABCDE
    my $run_name = random_value(6) . "_M" . random_value(5) . "_" . random_value(4) . "_0000000000-" . random_string(5);
    print "INSERT INTO run (name,date,assay,chemistry,description,investigator,path) VALUES(";
    print "'$run_name', '2016-03-01', 'Some Assay', 'Amplicon', 'Some Description', 'JK', '/some/path');\n";
    for (1..$num_samples) {
        my $primer_set = $primersets[rand @primersets];
        my $name = $names[rand @names];
        my $dna_nr = random_value(2) . "-" . random_value(5);
        my $project = $projects[rand @projects];
        my $lims_id = int(rand 10000000);
        my $id = $next_id++;
        my $cells = int(rand 20000);


        print "INSERT INTO sample (run,name,dna_nr,project,lims_id,primer_set,id,cells) VALUES ";
        print "('$run_name', '$name', '$dna_nr', ";
        if(rand(100)>$undef_rate) {
            print "'$project'";
        } else {
            print "''";
        }
        #        print_sometimes $project, 0;
        print ",";
        print_sometimes $lims_id, 1;
        print ",'$primer_set',$id,";
        print_sometimes $cells, 1;
        print ");\n";

        for (1..$num_fastqs) {
            my $filename = "/mnt/foo/bar/" . random_string(8) . "/"
                . random_value(4) . "_" . $name . "_" . $primer_set
                . "_S" . random_value(2) . "_L001_R1_001.fastq.gz";
            print "INSERT INTO fastq (sample_id, filename) VALUES ";
            print "($id, '$filename');\n";
        }
    }
}


