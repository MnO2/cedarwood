use cedarwood::Cedar;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
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

fn main() {
    let f = File::open("./dict.txt").unwrap();
    let mut buf = BufReader::new(f);
    let _ = IndexBuilder::new().build(&mut buf).unwrap();
}
