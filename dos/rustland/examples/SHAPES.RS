// Structs, impl blocks, enums and match.

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Circle,
    Square,
    Triangle,
}

#[derive(Debug)]
struct Shape {
    kind: Kind,
    size: i32,
}

impl Shape {
    fn new(kind: Kind, size: i32) -> Shape {
        Shape { kind, size }
    }

    /// Area with pi ~ 355/113 (R++ has no floating point).
    fn area(&self) -> i32 {
        match self.kind {
            Kind::Circle => self.size * self.size * 355 / 113,
            Kind::Square => self.size * self.size,
            Kind::Triangle => self.size * self.size / 2,
        }
    }

    fn grow(&mut self, by: i32) {
        self.size += by;
    }
}

fn describe(s: &Shape) -> &str {
    match s.area() {
        0..=10 => "tiny",
        11..=100 => "medium",
        _ => "huge",
    }
}

fn main() {
    let mut shapes = [
        Shape::new(Kind::Circle, 3),
        Shape::new(Kind::Square, 4),
        Shape::new(Kind::Triangle, 20),
    ];
    for s in &shapes {
        println!("{:?} of size {} has area {} ({})", s.kind, s.size, s.area(), describe(s));
    }
    shapes[0].grow(10);
    println!("{:?}", shapes[0]);
    println!("after growing: area {}", shapes[0].area());
}
