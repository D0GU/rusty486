// Guess the number: reading input and random numbers.

fn main() {
    let secret = random_range(1, 101);
    println!("I'm thinking of a number between 1 and 100.");
    let mut tries = 0;
    loop {
        print!("Your guess: ");
        let mut buf = [0u8; 16];
        let n = read_line(&mut buf);
        let guess = parse_int(str_from_bytes(&buf[..n]));
        tries += 1;
        if guess < secret {
            println!("Too small!");
        } else if guess > secret {
            println!("Too big!");
        } else {
            println!("You got it in {} tries!", tries);
            break;
        }
    }
}
