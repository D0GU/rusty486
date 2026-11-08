// Hello from R++! Press Ctrl+F9 to compile and run, Alt+F5 to see the
// output again, F1 for help.

fn main() {
    println!("Hello from Rustland R++!");
    let name = args();
    if !name.is_empty() {
        println!("Arguments: {}", name);
    }
    let mut total = 0;
    for i in 1..=10 {
        total += i;
    }
    println!("1 + 2 + ... + 10 = {}", total);
}
