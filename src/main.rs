use cosmic_text::{
    Attrs, Buffer, CacheKey, Color, Command, FontSystem, Metrics, Shaping, SwashCache, Transform,
};

use clap::Parser;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// String to parse into text
    text: String,
    output_file: String,
}

fn main() {
    let args = Args::parse();

    // A FontSystem provides access to detected system fonts, create one per application
    let mut font_system = FontSystem::new();

    // A SwashCache stores rasterized glyphs, create one per application
    let mut swash_cache = SwashCache::new();

    // Text metrics indicate the font size and line height of a buffer
    let metrics = Metrics::new(14.0, 20.0);

    // A Buffer provides shaping and layout for a UTF-8 string, create one per text widget
    let mut buffer = Buffer::new(&mut font_system, metrics);

    // Borrow buffer together with the font system for more convenient method calls
    let mut buffer = buffer.borrow_with(&mut font_system);

    // Set a size for the text buffer, in pixels
    buffer.set_size(Some(100000.0), Some(25.0));

    // Attributes indicate what font to choose
    let attrs = Attrs::new();

    // Add some text!
    buffer.set_text(&args.text, attrs, Shaping::Advanced);

    // Perform shaping as desired
    buffer.shape_until_scroll(true);
    let mut symbols: Vec<(i32, i32, CacheKey)> = vec![];

    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            let physical_glyph = glyph.physical((0., 0.), 1.0);

            let x = physical_glyph.x;
            let y = run.line_y as i32 + physical_glyph.y;

            symbols.push((x, y, physical_glyph.cache_key));
        }
    }

    let mut shapes = vec![];

    for (x, y, key) in symbols {
        let commands: Vec<_> = swash_cache
            .get_outline_commands(&mut font_system, key)
            .expect(format!("Expected a list of commands for character").as_ref())
            .iter()
            .map(|v| v.transform(&Transform::translation(x as f32, y as f32)))
            .collect();

        let mut last_point: Option<Point> = None;
        let mut first_point: Option<Point> = None;

        let mut primitives = vec![];

        if commands.len() == 0 {
            continue;
        }

        for command in commands {
            match command {
                Command::MoveTo(end_point) => {
                    first_point.get_or_insert(Point(end_point.x, end_point.y));
                    last_point = Some(Point(end_point.x, end_point.y))
                }
                Command::QuadTo(ctrl_point0, end_point) => {
                    let from_point = last_point.expect("Cannot QuadTo without a previous point");
                    let end_point = Point(end_point.x, end_point.y);
                    primitives.push(Primitive::Quadratic(
                        from_point,
                        Point(ctrl_point0.x, ctrl_point0.y),
                        end_point.clone(),
                    ));
                    last_point = Some(end_point);
                }
                Command::CurveTo(ctrl_point0, ctrl_point1, end_point) => {
                    let from_point = last_point.expect("Cannot CurveTo without a previous point");
                    let end_point = Point(end_point.x, end_point.y);
                    primitives.push(Primitive::Bezier(
                        from_point,
                        Point(ctrl_point0.x, ctrl_point0.y),
                        Point(ctrl_point1.x, ctrl_point1.y),
                        end_point.clone(),
                    ));
                    last_point = Some(end_point);
                }
                Command::LineTo(end_point) => {
                    let from_point = last_point.expect("Cannot LineTo without a previous point");
                    let end_point = Point(end_point.x, end_point.y);
                    primitives.push(Primitive::Line(from_point, end_point.clone()));
                    last_point = Some(end_point);
                }
                Command::Close => {
                    let from_point = last_point.expect("Cannot LineTo without a previous point");
                    let end_point = first_point
                        .take()
                        .expect("Cannot \"Close\" without a starting point");
                    primitives.push(Primitive::Line(from_point, end_point.clone()));
                    last_point = Some(end_point);
                }
            }
        }

        let s = Shape { primitives };

        shapes.push(s);
    }

    let (min_point, max_point) = shapes.first().expect("Geometry has no shapes").get_bb();
    let points = shapes
        .iter()
        .map(|s| s.get_bb())
        .fold((min_point, max_point), |(min_p, max_p), (p0, p1)| {
            (min_p.min(&p0), max_p.max(&p1))
        });
    shapes = shapes
        .into_iter()
        .map(|shape| shape.remap_shape(&points.0, &points.1))
        .collect();

    let out = serde_json::to_string(&shapes).expect("to be able to serialize shape");
    let mut file =
        std::fs::File::create(args.output_file).expect("To be able to create output file");
    file.write(&out.into_bytes())
        .expect("To be able to write into file");
}

use serde::Serialize;
use std::io::Write;

#[derive(Clone, Debug, Serialize)]
struct Point(f32, f32);

impl Point {
    fn map_scale(self, min_point: &Point, max_point: &Point) -> Self {
        let x = (self.0 - min_point.0) / (max_point.1 - min_point.1);
        let y = (self.1 - min_point.1) / (max_point.1 - min_point.1);
        Point(
            ((x * 1000.0 as f32) as i32) as f32 / 1000.0,
            ((y * 1000.0 as f32) as i32) as f32 / 1000.0,
        )
    }

    fn min(&self, other: &Point) -> Point {
        Point(f32::min(self.0, other.0), f32::min(self.1, other.1))
    }

    fn max(&self, other: &Point) -> Point {
        Point(f32::max(self.0, other.0), f32::max(self.1, other.1))
    }
}

#[derive(Debug, Serialize)]
enum Primitive {
    Quadratic(Point, Point, Point),
    Bezier(Point, Point, Point, Point),
    Line(Point, Point),
}

#[derive(Debug, Serialize)]
struct Shape {
    primitives: Vec<Primitive>,
}

impl Shape {
    fn get_bb(&self) -> (Point, Point) {
        let mut points = vec![];
        for p in self.primitives.iter() {
            match p {
                Primitive::Quadratic(p1, p2, p3) => {
                    points.push(p1);
                    points.push(p2);
                    points.push(p3);
                }
                Primitive::Bezier(p1, p2, p3, p4) => {
                    points.push(p1);
                    points.push(p2);
                    points.push(p3);
                    points.push(p4);
                }
                Primitive::Line(p1, p2) => {
                    points.push(p1);
                    points.push(p2);
                }
            }
        }

        let min_x = points
            .iter()
            .map(|point| point.0)
            .min_by(|a, b| a.partial_cmp(b).expect("Unable to order floats"))
            .expect("No items found");

        let min_y = points
            .iter()
            .map(|point| point.1)
            .min_by(|a, b| a.partial_cmp(b).expect("Unable to order floats"))
            .expect("No items found");

        let max_x = points
            .iter()
            .map(|point| point.0)
            .max_by(|a, b| a.partial_cmp(b).expect("Unable to order floats"))
            .expect("No items found");

        let max_y = points
            .iter()
            .map(|point| point.1)
            .max_by(|a, b| a.partial_cmp(b).expect("Unable to order floats"))
            .expect("No items found");

        (Point(min_x, min_y), Point(max_x, max_y))
    }

    fn remap_shape(self, min_point: &Point, max_point: &Point) -> Self {
        let primitives = self
            .primitives
            .into_iter()
            .map(|primitive| match primitive {
                Primitive::Quadratic(p1, p2, p3) => Primitive::Quadratic(
                    p1.map_scale(&min_point, &max_point),
                    p2.map_scale(&min_point, &max_point),
                    p3.map_scale(&min_point, &max_point),
                ),
                Primitive::Bezier(p1, p2, p3, p4) => Primitive::Bezier(
                    p1.map_scale(&min_point, &max_point),
                    p2.map_scale(&min_point, &max_point),
                    p3.map_scale(&min_point, &max_point),
                    p4.map_scale(&min_point, &max_point),
                ),
                Primitive::Line(p1, p2) => Primitive::Line(
                    p1.map_scale(&min_point, &max_point),
                    p2.map_scale(&min_point, &max_point),
                ),
            })
            .collect();

        Self { primitives }
    }
}
