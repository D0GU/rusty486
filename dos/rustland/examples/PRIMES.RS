// The sieve of Eratosthenes, with arrays, slices and formatting.

const LIMIT: usize = 200;

fn count(flags: &[bool]) -> u32 {
    let mut n = 0;
    for f in flags {
        if *f {
            n += 1;
        }
    }
    n
}

fn main() {
    let mut prime = [true; LIMIT];
    prime[0] = false;
    prime[1] = false;
    let mut i = 2;
    while i * i < LIMIT {
        if prime[i] {
            let mut j = i * i;
            while j < LIMIT {
                prime[j] = false;
                j += i;
            }
        }
        i += 1;
    }
    println!("Primes below {}:", LIMIT);
    let mut col = 0;
    for n in 0..LIMIT {
        if prime[n] {
            print!("{:5}", n);
            col += 1;
            if col % 10 == 0 {
                println!();
            }
        }
    }
    println!();
    println!("{} primes, {} below 100", count(&prime), count(&prime[..100]));
}
