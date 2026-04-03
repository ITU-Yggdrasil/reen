# Agent Types for Specification Creation

The reen system uses three different specification agents based on the type of draft being processed. This ensures that specifications are tailored to the specific nature of each component type.

## Agent Selection

The system automatically selects the appropriate agent based on the file's location in the `drafts/` folder:

### 1. Data Type Agent (`create_specifications_data`)

**Used for**: `drafts/data/*.md`

**Purpose**: Creates specifications for simple data types with no behavior or role players.

**Characteristics**:
- Simple structs or enums with fields/variants
- **IMMUTABLE by default** - all fields are read-only unless explicitly documented as mutable
- NO methods beyond basic constructors and getters
- NO setters (unless mutability is explicitly justified)
- NO role players or actors
- NO use cases or sequence diagrams
- NO complex interactions between fields
- Pure data containers with validation rules

**Output includes**:
- Description - what the type represents and its purpose
- Type kind (Struct/Enum/NewType)
- **Mutability contract** (Immutable by default)
- **Fields** or **Variants** - the data structure
- **Functionalities** - the public API (or omitted for default: constructor + getters)
- Validation rules
- Examples (valid and invalid cases with constructor calls)
- Serialization requirements

**Does NOT include**:
- Methods beyond what's in Functionalities section
- Use cases or scenarios
- Sequence diagrams
- Role players or actors

**Key Structure**:
- **Fields** / **Variants** define the data
- **Functionalities section** defines the public API
  - If omitted: default to constructor + getters
  - If present: implement ONLY what's listed (must include constructor if needed)

### 2. Context Agent (`create_specifications_context`)

**Used for**: `drafts/contexts/*.md`

**Purpose**: Creates specifications for contexts with role players, use cases, and interactions.

**Characteristics**:
- Contains role players (objects acting as actors)
- Defines use cases and scenarios
- Includes interactions between entities
- Uses sequence diagrams
- Documents business rules

**Output includes**:
- Props (context properties)
- Roles and responsibilities
- Role players and their capabilities
- Functionalities (public operations)
- Use cases
- Sequence diagrams
- Business rules
- Examples

### 3. Root Application Drafts (`drafts/app.md`)

**Used for**: `drafts/app.md` (root application draft files)

**Handled by**: `create_specifications_context` with `specification_kind = app`

**Purpose**: Creates specifications for root application entry points without forcing them into the context/use-case schema.

**Characteristics**:
- Application entry points (binary, service, or web app)
- May declare an explicit application kind such as `cli_app`, `service_app`, or `web_app`
- Top-level startup/bootstrap and shutdown behavior
- Configuration, collaborator wiring, and lifecycle rules
- Optional command interface, transport surface, and static surface sections

**Output includes**:
- Application kind when present in the draft
- Runtime topology and application flow
- Configuration surface
- Command interface when the draft defines args/subcommands/flags
- Transport surface when the draft defines routes/endpoints
- Static surface when the draft defines pages/assets
- Collaborators and wiring
- Error handling, exit, and shutdown behavior

**Does NOT include**:
- Detailed context implementations (those go in context specs)
- Low-level data structures (those go in data specs)
- Forced role-player structure for app drafts that are written as lifecycle/configuration documents

## Processing Order

Files are processed in this order to ensure dependencies are available:

1. **Data types first** (`data/` folder)
   - Simple types with no dependencies

2. **Contexts second** (`contexts/` folder)
   - May depend on data types

3. **Root app files last** (root folder)
   - May depend on both data types and contexts

## File Structure Mapping

```
drafts/
├── data/
│   └── X.md → create_specifications_data → specifications/data/X.md
├── contexts/
│   └── Y.md → create_specifications_context → specifications/contexts/Y.md
└── app.md → create_specifications_context (app mode) → specifications/app.md
```

## Implementation Impact

When implementing specifications, the same folder-based selection applies:

- **Data specs** → Simple type implementations (structs/enums)
  - **All fields are private**
  - **Public getters only** (no setters by default)
  - **Immutable** unless specification explicitly documents mutability
  - Derives: `Debug`, `Clone`, `PartialEq`, `Eq` (as appropriate)

- **Context specs** → Context implementations with role methods
  - Struct with role players and props as fields
  - Public methods from "Functionalities"
  - Private role methods from "Role Methods"

- **Root app specs** → Application entry points (for example `src/main.rs`)
  - Startup/bootstrap flow
  - Collaborator wiring
  - Command interface when documented
  - Transport/static surfaces when documented

The implementation agent (`create_implementation`) enforces immutability for data types:
- Validates that mutability is explicitly justified if present
- Creates private fields with public getters
- NO setters unless specification documents why mutability is needed
