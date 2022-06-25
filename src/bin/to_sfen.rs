use std::{env::args, process::exit};

#[derive(Default)]
struct Opts {
    verbose: bool,
}

fn fetch_problem(url: &str, opts: &Opts) -> String {
    let mut url = url.to_string();
    if url.starts_with("https://www.shogi.or.jp/tsume_shogi/") && url.ends_with(".html") {
        let response = tinyget::get(url).send().unwrap();
        let str = response.as_str().unwrap();
        let x = str
            .find("https://www.shogi.or.jp/tsume_shogi/data/")
            .unwrap();
        let slice = &str[x..];
        let y = slice.find(".kif").unwrap();
        url = slice[..y + 4].to_string();
        if opts.verbose {
            eprintln!("url = {}", url);
        }
    }
    let response = tinyget::get(url).send().unwrap();
    let bytes = response.into_bytes();
    let (cow, _, failure) = encoding_rs::SHIFT_JIS.decode(&bytes);
    if !failure {
        return cow.into_owned();
    }
    todo!();
}

fn main() {
    let opts = Opts {
        verbose: true,
        ..Default::default()
    };
    let args: Vec<_> = args().collect();
    if args.len() <= 1 {
        exit(1);
    }
    let filename_or_url = args[1].to_string();
    let mut data = String::new();
    if filename_or_url.starts_with("http://") || filename_or_url.starts_with("https://") {
        data = fetch_problem(&filename_or_url, &opts);
    }
    if opts.verbose {
        eprintln!("data = {}", data);
    }
    let record_type = shogi_mate_solver::check_record_type(&data);
    eprintln!("record_type = {:?}", record_type);
}
