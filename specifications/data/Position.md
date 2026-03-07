### Position

#### Description
A value object representing a cell coordinate on the Board.

#### Type Kind
Struct

#### Mutability
Immutable

#### Properties
- **x_** - an integer >= 0 representing the x-coordinate on the cell grid
- **y_** - an integer >= 0 representing the y-coordinate on the cell grid

#### Functionalities
- **eq** -> supports equality. Two positions are considered equal if x1 equals x2 and y1 equals y2.

#### Constraints & Rules
- None explicitly stated.

**Inferred Types or Structures (Non-Blocking)**
- **Properties** `x_` and `y_` are assumed to be inferred from the Board's coordinate system.

**Blocking Ambiguities**
- None.

**Implementation Choices Left Open**
- Exact integer type for `x_` and `y_` defaults to `i32`.
- The `eq` method uses Rust's `PartialEq` trait.
- Auto-implemented getters for `x` and `y` are provided.

---

### Position

#### Description
A value object representing a cell coordinate on the Board.

#### Type Kind
Struct

#### Mutability
Immutable

#### Properties
- **x_** - an integer >= 0 representing the x-coordinate on the cell grid
- **y_** - an integer >= 0 representing the y-coordinate on the cell grid

#### Functionalities
- **eq** -> supports equality. Two positions are considered equal if x1 equals x2 and y1 equals y2.

#### Constraints & Rules
- None explicitly stated.

**Inferred Types or Structures (Non-Blocking)**
- **Properties** `x_` and `y_` are assumed to be inferred from the Board's coordinate system.

**Blocking Ambiguities**
- None.

**Implementation Choices Left Open**
- Exact integer type for `x_` and `y_` defaults to `i32`.
- The `eq` method uses Rust's `PartialEq` trait.
- Auto-implemented getters for `x` and `y` are provided.