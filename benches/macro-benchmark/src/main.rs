use cedarwood::Cedar;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::env;
use std::time;

struct IndexBuilder {}

impl IndexBuilder {
    pub fn new() -> Self {
        IndexBuilder {}
    }

    // Require the dictionary to be sorted in lexicographical order
    pub fn build<R: BufRead>(&mut self, dict: &mut R) -> io::Result<Cedar> {
        let mut buf = String::new();
        let mut records: Vec<(String, usize, String)> = Vec::new();

        while dict.read_line(&mut buf)? > 0 {
            {
                let parts: Vec<&str> = buf.trim().split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }

                let word = parts[0];
                let freq = parts.get(1).map(|x| x.parse::<usize>().unwrap()).unwrap_or(0);
                let tag = parts.get(2).cloned().unwrap_or("");

                records.push((String::from(word), freq, String::from(tag)));
            }
            buf.clear();
        }

        let dict: Vec<&str> = records.iter().map(|n| n.0.as_ref()).collect();
        let key_values: Vec<(&str, i32)> = dict.into_iter().enumerate().map(|(k, s)| (s, k as i32)).collect();

        let now = time::Instant::now();
        let mut cedar = Cedar::new();
        cedar.build(&key_values);
        println!("{} ms", now.elapsed().as_millis());

        Ok(cedar)
    }
}

pub fn query<R: BufRead>(dict: &mut R, cedar: &Cedar) -> io::Result<()> {
    let mut buf = String::new();
    let mut records: Vec<(String, usize, String)> = Vec::new();

    while dict.read_line(&mut buf)? > 0 {
        {
            let parts: Vec<&str> = buf.trim().split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            let word = parts[0];
            let freq = parts.get(1).map(|x| x.parse::<usize>().unwrap()).unwrap_or(0);
            let tag = parts.get(2).cloned().unwrap_or("");

            records.push((String::from(word), freq, String::from(tag)));
        }
        buf.clear();
    }

    let dict: Vec<&str> = records.iter().map(|n| n.0.as_ref()).collect();
    let keys: Vec<&str> = dict.into_iter().enumerate().map(|(_, s)| s).collect();

    let now = time::Instant::now();
    for k in keys {
        cedar.exact_match_search(k);
    }
    println!("{} ms", now.elapsed().as_millis());

    Ok(())
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("bench <dict> <query>");
        std::process::exit(1);
    }

    let f = File::open(&args[1])?;
    let mut buf = BufReader::new(f);
    let cedar = IndexBuilder::new().build(&mut buf).unwrap();

    let f = File::open(&args[2])?;
    let mut buf = BufReader::new(f);
    query(&mut buf, &cedar)?;

    Ok(())
}
