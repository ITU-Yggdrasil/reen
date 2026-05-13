# Agent Types for Specification Creation

The reen system uses multiple specialized specification agents based on the type of draft being processed. This ensures that specifications are tailored to the specific nature of each component type.

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
- **Properties** - the data structure (fields/variants)
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
- **Properties section** defines the data (becomes private fields)
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
- Functionality (public operations)
- Use cases
- Sequence diagrams
- Business rules
- Examples

### 3. Main Application Agent (`create_specifications_main`)

**Used for**: `drafts/*.md` (root folder files like `app.md`)

**Purpose**: Creates specifications for main application entry points (main.rs, mod.rs, or library roots).

**Characteristics**:
- Application entry points (binary or library root)
- Command-line interface structure
- Top-level application flow
- Module organization
- Configuration and initialization

**Output includes**:
- Application overview (type: Binary/Library/Module)
- Command structure (for CLI apps)
- Application flow
- Module organization
- Configuration requirements
- Dependencies
- Error handling strategy
- Usage examples

**Does NOT include**:
- Detailed context implementations (those go in context specs)
- Low-level data structures (those go in data specs)
- Role player interactions (those go in context specs)

### 4. Brand Identity Agent (`create_specifications_brand`)

**Used for**: `drafts/brands/*.md`, `drafts/visuals/*.md`

**Purpose**: Creates canonical markdown brand identity specifications from designer drafts.

**Characteristics**:
- Outputs a structured markdown specification with a fixed section order
- Defines reusable visual primitives
- Covers colors, typography, imagery, iconography, motion, and layout
- Is implementation-independent and component-independent
- Serves as the authoritative token source for downstream specs
- Surfaces missing required primitive families as explicit blocking ambiguities

**Output includes**:
- `# Brand Identity Specification`
- `## Description`
- `## Brand Metadata`
- `## Color Tokens`
- `## Typography`
- `## Iconography`
- `## Motion`
- `## Layout Principles`
- `## Token Reference Rules`
- `## Blocking Ambiguities` when applicable
- `## Implementation Choices Left Open` when applicable

**Does NOT include**:
- Component styling
- Framework-specific code
- Arbitrary section structure outside the canonical contract

### 5. Layout Specification Agent (`create_specifications_layout`)

**Used for**: `drafts/layouts/*.md`

**Purpose**: Creates deterministic page layout specifications from page-blueprint drafts.

**Characteristics**:
- Outputs a structured page-level composition contract
- Defines section order, section purpose, visual priority, and content mode
- Declares which components appear in each section and how many there are
- Is implementation-independent and distinct from reusable component APIs
- Uses strict component-name matching against known component context

**Output includes**:
- `# Page Layout Specification`
- `## Description`
- `## Page Metadata`
- `## Visual Intent`
- `## Section Order`
- `## Sections`
- `## Component Placement Summary`
- `## Blocking Ambiguities` when applicable
- `## Implementation Choices Left Open` when applicable

**Does NOT include**:
- Reusable component prop APIs
- Framework-specific code
- Brand-token definitions
- Arbitrary page sections not justified by the draft

## Processing Order

Files are processed in this order to ensure dependencies are available:

1. **Data types first** (`data/` folder)
   - Simple types with no dependencies

2. **Contexts second** (`contexts/` folder)
   - May depend on data types

3. **Brands / visuals as foundational design artifacts** (`brands/`, `visuals/` folders)
   - May be referenced by downstream site work

4. **Layouts as page blueprints** (`layouts/` folder)
   - May depend on existing brand/visual and component context

5. **Main files last** (root folder)
   - May depend on both data types and contexts

## File Structure Mapping

```
drafts/
|- data/
|  `- X.md -> create_specifications_data -> specifications/data/X.md
|- contexts/
|  `- Y.md -> create_specifications_context -> specifications/contexts/Y.md
|- brands/
|  `- Z.md -> create_specifications_brand -> specifications/brands/Z.md
|- visuals/
|  `- V.md -> create_specifications_brand -> specifications/visuals/V.md
|- layouts/
|  `- L.md -> create_specifications_layout -> specifications/layouts/L.md
`- app.md -> create_specifications_main -> specifications/app.md
```

## Implementation Impact

When implementing specifications, the same folder-based selection applies:

- **Data specs** -> Simple type implementations (structs/enums)
  - **All fields are private**
  - **Public getters only** (no setters by default)
  - **Immutable** unless specification explicitly documents mutability
  - Derives: `Debug`, `Clone`, `PartialEq`, `Eq` (as appropriate)

- **Context specs** -> Context implementations with role methods
  - Struct with role players and props as fields
  - Public methods from "Functionality"
  - Private role methods from "Role Methods"

- **Main specs** -> Application entry points (main.rs or lib.rs)
  - CLI argument parsing
  - Module organization
  - Application flow

- **Layout specs** -> Page composition inputs for site implementation
  - Section order
  - Component placement
  - Repeated-item counts
  - Action ownership

- **Visual specs** + **layout specs** + **component specs** -> Brand-site implementation scaffold
  - Visual specs define identity and styling direction
  - Layout specs define the page blueprint
  - Component specs define reusable building blocks

The implementation agent (`create_implementation`) enforces immutability for data types:
- Validates that mutability is explicitly justified if present
- Creates private fields with public getters
- NO setters unless specification documents why mutability is needed
