use crate::parser::{IgdtBowl, Measure, Recipe, Stmt, StmtKind};
use crate::{RChefError, Result};

use if_chain::if_chain;
use num_bigint::BigInt;
use rand::prelude::SliceRandom;

use std::collections::HashMap;
use std::convert::TryFrom;
use std::convert::TryInto;
use std::io::{self, Write};
use std::num::NonZeroU32;

pub fn run(recipes: Vec<Recipe>, spaced: bool) -> Result<()> {
    Interpreter::new(recipes, spaced)?.run()
}

#[derive(Debug)]
pub struct Interpreter {
    recipes: HashMap<String, Recipe>,
    main: String,
    spaced: bool,
}

impl Interpreter {
    pub fn new(recipes: Vec<Recipe>, spaced: bool) -> Result<Self> {
        if recipes.is_empty() {
            crate::report_error(0, "syntax ", "programs must contain at least one recipe");
            return Err(RChefError::Runtime);
        }

        let main = recipes[0].title.to_lowercase();
        let recipes = recipes
            .into_iter()
            .map(|rec| (rec.title.to_lowercase(), rec))
            .collect();

        Ok(Self {
            recipes,
            main,
            spaced,
        })
    }

    pub fn run(self) -> Result<()> {
        let main = self.recipes.get(&self.main).unwrap();

        RecipeRunner::new(&self, main).run()?;
        Ok(())
    }

    #[cfg(test)]
    pub fn run_and_return_state(&self) -> Result<RecipeRunner> {
        let main = self.recipes.get(&self.main).unwrap();
        let mut runner = RecipeRunner::new(&self, main);
        runner.execute()?;

        Ok(runner)
    }
}

#[derive(Debug)]
pub struct RecipeRunner<'a> {
    pub interpreter: &'a Interpreter,
    pub recipe: &'a Recipe,
    pub ingredients: Option<HashMap<String, Option<Value>>>,
    pub bowls: HashMap<NonZeroU32, ValueStack>,
    pub dishes: HashMap<NonZeroU32, ValueStack>,
    pub only_first_bowl: bool,
    pub only_first_dish: bool,
    pub line: u32,
    pub refrigerated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Value {
    pub num: BigInt,
    pub measure: Measure,
}

#[derive(Clone, Debug)]
pub struct ValueStack {
    pub values: Vec<Value>,
}

impl ValueStack {
    fn new() -> Self {
        Self { values: vec![] }
    }

    fn push(&mut self, val: Value) {
        self.values.push(val);
    }

    fn pop(&mut self) -> Option<Value> {
        self.values.pop()
    }

    fn stir(&mut self, n: usize) {
        if n == 0 {
            return;
        }

        let len = self.values.len();
        let n = n.min(len - 1);

        self.values[len - 1 - n..].rotate_right(1);
    }

    fn mix(&mut self) {
        let mut rng = rand::thread_rng();
        self.values.shuffle(&mut rng);
    }

    fn clean(&mut self) {
        self.values.clear();
    }

    fn pour_into(&self, other: &mut ValueStack) {
        other.values.extend_from_slice(&self.values[..]);
    }

    fn printable(&self, spaced: bool) -> String {
        let mut buffer = String::new();

        for val in self.values.iter().rev() {
            match val.measure {
                Measure::Dry | Measure::Ambiguous => {
                    if spaced && !buffer.is_empty() && !buffer.ends_with(' ') {
                        buffer.push(' ');
                    }

                    buffer.push_str(&val.num.to_string());

                    if spaced {
                        buffer.push(' ');
                    }
                }
                Measure::Liquid => {
                    if_chain! {
                        if let Ok(num) = u32::try_from(&val.num);
                        if let Some(ch) = char::from_u32(num);
                        then {
                            buffer.push(ch);
                        } else {
                            buffer.push(char::REPLACEMENT_CHARACTER);
                        }
                    }
                }
            }
        }

        buffer
    }
}

impl<'a> RecipeRunner<'a> {
    fn new(interpreter: &'a Interpreter, recipe: &'a Recipe) -> Self {
        let ingredients = recipe.ingredients.as_ref().map(|vec| {
            vec.iter()
                .map(|ing| {
                    (
                        ing.name.clone(),
                        ing.initial_value.as_ref().map(|num| Value {
                            num: num.clone(),
                            measure: ing.measure,
                        }),
                    )
                })
                .collect()
        });

        Self {
            interpreter,
            recipe,
            ingredients,
            bowls: HashMap::new(),
            dishes: HashMap::new(),
            only_first_bowl: true,
            only_first_dish: true,
            line: 0,
            refrigerated: false,
        }
    }

    fn with_bowls_dishes(
        interpreter: &'a Interpreter,
        recipe: &'a Recipe,
        bowls: HashMap<NonZeroU32, ValueStack>,
        dishes: HashMap<NonZeroU32, ValueStack>,
    ) -> Self {
        Self {
            bowls,
            dishes,
            ..Self::new(interpreter, recipe)
        }
    }

    fn execute(&mut self) -> Result<()> {
        for stmt in &self.recipe.method {
            self.line = stmt.line;
            self.execute_stmt(stmt)?;
            if self.refrigerated {
                break;
            }
        }

        if !self.refrigerated {
            if let Some(n) = self.recipe.serves {
                self.print(n);
            }
        }

        Ok(())
    }

    fn run(mut self) -> Result<Option<ValueStack>> {
        self.execute()?;

        Ok(self.bowls.remove(&NonZeroU32::new(1).unwrap()))
    }

    fn execute_stmt(&mut self, stmt: &Stmt) -> Result<()> {
        use StmtKind::*;

        match &stmt.kind {
            Take(ing) => {
                let ing = self.get_ingredient_mut(ing)?;
                let mut buffer = String::new();
                print!("enter a number: ");
                io::stdout().flush()?;
                io::stdin().read_line(&mut buffer)?;

                if let Ok(num) = buffer.trim().parse() {
                    *ing = Some(Value {
                        num,
                        measure: Measure::Dry,
                    });
                } else {
                    return self.error("could not read number from stdin");
                };
            }
            Put(IgdtBowl(igdt, bowl)) => {
                let igdt = self.get_ingredient(igdt)?.clone();
                let bowl = self.get_bowl_mut(bowl)?;

                bowl.push(igdt);
            }
            Fold(IgdtBowl(igdt, bowl)) => {
                let val = if let Some(val) = self.get_bowl_mut(bowl)?.pop() {
                    val
                } else {
                    return self.error(format!(
                        "attempted to get value from bowl {}, but it's empty!",
                        bowl.map(NonZeroU32::get).unwrap_or(1)
                    ));
                };

                *self.get_ingredient_mut(igdt)? = Some(val);
            }
            Add(x) => self.arithmetic_helper(x, |a, b| a + b)?,
            Remove(x) => self.arithmetic_helper(x, |a, b| b - a)?,
            Combine(x) => self.arithmetic_helper(x, |a, b| a * b)?,
            Divide(x) => self.arithmetic_helper(x, |a, b| b / a)?,
            AddDry(bowl) => {
                let num = if let Some(ingredients) = &self.ingredients {
                    ingredients
                        .values()
                        .flatten()
                        .filter_map(|val| {
                            if val.measure == Measure::Dry {
                                Some(&val.num)
                            } else {
                                None
                            }
                        })
                        .sum()
                } else {
                    return self
                        .error("attempted to access ingredients, but recipe has no ingredients!");
                };

                self.get_bowl_mut(bowl)?.push(Value {
                    num,
                    measure: Measure::Dry,
                });
            }
            Liquefy(name) => {
                let igdt = self.get_ingredient_mut(name)?;

                if let Some(igdt) = igdt.as_mut() {
                    igdt.measure = Measure::Liquid;
                } else {
                    return self
                        .error(format!("attempted to use the value of ingredient '{}', but ingredient is uninitialized!", name));
                }
            }
            LiquefyConts(bowl) => {
                let bowl = self.get_bowl_mut(bowl)?;

                bowl.values
                    .iter_mut()
                    .for_each(|val| val.measure = Measure::Liquid);
            }
            Stir(bowl, n) => self.get_bowl_mut(bowl)?.stir(*n),

            StirInto(IgdtBowl(name, bowl)) => {
                let igdt = self.get_ingredient(name)?;
                let n: usize = if let Ok(n) = (&igdt.num).try_into() {
                    n
                } else {
                    return self.error(format!(
                        "'{}'s value ({}) is too large for stirring!",
                        name, igdt.num
                    ));
                };

                self.get_bowl_mut(bowl)?.stir(n);
            }
            Mix(bowl) => self.get_bowl_mut(bowl)?.mix(),

            Clean(bowl) => self.get_bowl_mut(bowl)?.clean(),
            Pour(bowl, dish) => {
                let bowlkey =
                    Self::bowldish_key_helper(self.line, "bowl", bowl, &mut self.only_first_bowl)?;
                let bowl = if let Some(bowl) = self.bowls.get(&bowlkey) {
                    bowl
                } else {
                    return Ok(());
                };

                let dish = Self::get_bowl_or_dish_mut(
                    self.line,
                    &mut self.dishes,
                    "dish",
                    dish,
                    &mut self.only_first_dish,
                )?;

                bowl.pour_into(dish);
            }
            Loop {
                igdt1,
                igdt2,
                stmts,
            } => 'outer: loop {
                let igdt1 = self.get_ingredient(igdt1)?;
                if igdt1.num == 0.into() {
                    break;
                }

                for stmt in stmts {
                    match stmt.kind {
                        SetAside => break 'outer,
                        _ => {}
                    }
                    self.execute_stmt(stmt)?;
                    if self.refrigerated {
                        break 'outer;
                    }
                }

                if let Some(name) = igdt2 {
                    let line = self.line;
                    if let Some(igdt2) = self.get_ingredient_mut(name)? {
                        igdt2.num -= 1;
                    } else {
                        Self::error_with_line(
                            line,
                            format!(
                                "attempted to access ingredient '{}', but recipe has no such ingredient!",
                                name
                            ),
                        );
                        return Err(RChefError::Runtime);
                    }
                }
            },
            SetAside => unreachable!(), // this is unreachable in the way that the grammar is parsed
            ServeWith(recipe) => {
                let recipe =
                    if let Some(recipe) = self.interpreter.recipes.get(&recipe.to_lowercase()) {
                        recipe
                    } else {
                        return self.error(format!("couldn't find the recipe {}", recipe));
                    };
                let bowl = RecipeRunner::with_bowls_dishes(
                    self.interpreter,
                    recipe,
                    self.bowls.clone(),
                    self.dishes.clone(),
                )
                .run()?;

                if let Some(outbowl) = bowl {
                    let bowl = self.get_bowl_mut(&Some(NonZeroU32::new(1).unwrap()))?;
                    outbowl.pour_into(bowl);
                }
            }
            Refrigerate(n) => {
                self.refrigerated = true;
                if let Some(n) = *n {
                    self.print(n);
                }
            }
        }

        Ok(())
    }

    fn print(&self, n: NonZeroU32) {
        let mut buffer = String::new();
        for i in 1..=n.get() {
            let i = unsafe { NonZeroU32::new_unchecked(i) };

            if let Some(dish) = self.dishes.get(&i) {
                buffer.push_str(&dish.printable(self.interpreter.spaced));
            }
        }

        println!("{}", buffer);
    }

    fn arithmetic_helper(
        &mut self,
        igdtbowl: &IgdtBowl,
        f: impl FnOnce(&BigInt, &BigInt) -> BigInt,
    ) -> Result<()> {
        let IgdtBowl(igdt, bowl) = igdtbowl;

        let top = if let Some(val) = self.get_bowl_mut(bowl)?.pop() {
            val
        } else {
            return self.error("attempted to get value from bowl, but it was empty!");
        };
        let igdt = self.get_ingredient(igdt)?;
        let num = f(&igdt.num, &top.num);

        let bowl = self.get_bowl_mut(bowl)?;
        bowl.push(Value {
            num,
            measure: Measure::Dry,
        });

        Ok(())
    }

    fn get_bowl_mut(&mut self, key: &Option<NonZeroU32>) -> Result<&mut ValueStack> {
        Self::get_bowl_or_dish_mut(
            self.line,
            &mut self.bowls,
            "bowl",
            key,
            &mut self.only_first_bowl,
        )
    }

    fn get_bowl_or_dish_mut<'h>(
        line: u32,
        things: &'h mut HashMap<NonZeroU32, ValueStack>,
        thing_name: &'static str,
        key: &Option<NonZeroU32>,
        only_first: &mut bool,
    ) -> Result<&'h mut ValueStack> {
        let key = Self::bowldish_key_helper(line, thing_name, key, only_first)?;

        Ok(things.entry(key).or_insert_with(|| ValueStack::new()))
    }

    fn bowldish_key_helper(
        line: u32,
        thing_name: &'static str,
        key: &Option<NonZeroU32>,
        only_first: &mut bool,
    ) -> Result<NonZeroU32> {
        if let Some(key) = key {
            if key.get() != 1 {
                *only_first = true;
            }
            Ok(*key)
        } else if *only_first {
            Ok(NonZeroU32::new(1).unwrap())
        } else {
            Self::error_with_line(
                line,
                format!(
                    "attempted to access {0} without ordinal, yet more than one {0} is in use!",
                    thing_name
                ),
            );
            Err(RChefError::Runtime)
        }
    }

    fn get_ingredient(&self, ing: &str) -> Result<&Value> {
        let ingredients = if let Some(x) = self.ingredients.as_ref() {
            x
        } else {
            return self.error("attempted to access ingredient, but recipe has no ingredients!");
        };

        let val = if let Some(val) = ingredients.get(ing) {
            val
        } else {
            return self.error(format!(
                "attempted to access ingredient '{}', but recipe has no such ingredient!",
                ing
            ));
        };

        if let Some(val) = val.as_ref() {
            Ok(val)
        } else {
            self.error(format!(
                "attempted to use the value of ingredient '{}', but ingredient is uninitialized!",
                ing
            ))
        }
    }

    fn get_ingredient_mut(&mut self, ing: &str) -> Result<&mut Option<Value>> {
        let line = self.line;
        // ^ blame the borrow checker, not me
        // i would've done this much more simply but no, the borrow checker
        // can't figure out that i'm not using the mutable reference to self
        // when i call self.error()

        let ingredients = if let Some(x) = self.ingredients.as_mut() {
            x
        } else {
            Self::error_with_line(
                line,
                "attempted to access ingredient, but recipe has no ingredients!",
            );
            return Err(RChefError::Runtime);
        };

        if let Some(val) = ingredients.get_mut(ing) {
            Ok(val)
        } else {
            Self::error_with_line(
                line,
                format!(
                    "attempted to access ingredient '{}', but recipe has no such ingredient!",
                    ing
                ),
            );
            Err(RChefError::Runtime)
        }
    }

    fn report_error(&self, msg: impl std::fmt::Display) {
        Self::error_with_line(self.line, msg);
    }

    fn error<T, D: std::fmt::Display>(&self, msg: D) -> Result<T> {
        self.report_error(msg);
        Err(RChefError::Runtime)
    }

    fn error_with_line<D: std::fmt::Display>(line: u32, msg: D) {
        crate::report_error(line, "runtime ", msg);
    }
}
