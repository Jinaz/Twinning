# Digital Twin (DT) Implementation — Reference Architecture

This repository contains an implementation of a **Digital Twin** according to the **unifying DT reference model** proposed in *“Towards a Unifying Reference Model for Digital Twins of Cyber-Physical Systems”* (arXiv:2507.04871v1).  
Reference: https://arxiv.org/html/2507.04871v1

The goal is to provide a practical, extensible DT core that closes the “concept → implementation” gap by structuring the DT into well-defined building blocks (Engine, Gateways, Models, Data, Services) and explicit relations between them.

---

## Status

- ✅ Implementation follows the reference architecture (extracting structs and traits at the moment form implementation).
- 🚧 Examples and run demos are **Work in Progress (WIP)**.

---

## Reference Model at a Glance

The architecture centers around:

- **Digital Twin Engine** (core)
  - connects **Gateways**, **Models**, **Data**, and **Services**
  - contains a **Synchronizer** operating over **Mappings** between *Gateway Elements* and *Model Elements*
  - mappings can be **uni-/bi-directional** and synchronized by **frequency** or **trigger**

- **Digital Twin Gateway** (connects to the Actual System / “AS”)
  - represents one AS and exposes its features as **Gateway Elements**:
    - **Properties** (read/observe/synchronize)
    - **Events** (reactive notifications)
    - **Functions** (invokable operations / commands)

- **Models**
  - conform to a **Modeling Language**
  - are managed via **Model Managers**
  - can be used **offline** (not connected to a Gateway) or **online** (connected; can affect the AS)

- **Data**
  - persisted and accessed via a **Data Manager**
  - qualified with metadata / **Data Properties** (e.g., origin, precision, last update, uncertainty, …)
  - data is relatable to models

- **Services**
  - added-value functionality on top of the DT core representation/synchronization
  - exposed via explicit service interfaces (not “hidden inside” the DT core)

---

## Getting Started (Conceptual Workflow)

To create a DT instance with this implementation:

1. **Model your DB schema**  
   Define the persistence structure of the DT data (entities/tables/collections and relevant metadata).

2. **Model your data source**  
   Describe the upstream system/API/stream and its exposed **properties** and **events**.

3. **Model the mapping between schema and source**  
   Specify how source elements map to schema/model elements (including direction and synchronization strategy).

4. **Expose properties and events via services**  
   Make selected elements accessible through service interfaces (e.g., query endpoints, subscriptions, command invocations).

> Examples and runnable demos are currently WIP, but the architecture and extension points are already in place.

---

## Core Concepts & Terminology

### Actual System (AS)
The real-world system being twinned (device, machine, plant, vehicle, simulator, platform). The DT interacts through gateways.

### Gateway Elements
The DT-facing interface of the AS:

- **Property**: readable and/or observable value, used for synchronization  
- **Event**: notifications emitted by AS or gateway  
- **Function**: invokable capability on the AS (command with args/results)

### Models & Data
- **Models** capture structure/behavior/constraints and are managed via model managers.
- **Data** is persisted and qualified with metadata (origin, time, quality, etc.) and relates to models.

### Mappings
Mappings connect gateway elements ↔ model/data elements and define:
- **direction** (`AS -> DT`, `DT -> AS`, `AS <-> DT`)
- **strategy** (periodic vs. event-triggered)

---

## Extending the DT

### Add a new Gateway (protocol / platform)
Implement the gateway interface and map protocol-specific concepts onto:
- property
- event
- function

### Add a new Data Backend
Implement a data manager adapter supporting persistence and metadata/data-properties.

### Add a new Modeling Language
Provide:
- modeling language adapter (parse/serialize)
- model manager
- integrity-preserving model operations (optional but recommended)

### Add a Service
Services sit on top of the core DT to expose selected properties/events/functions and implement value-added capabilities.

---

## Design Principles

- **Explicit boundaries** between DT, AS, and services (avoid “everything is part of the DT” ambiguity).
- **Core ≠ Services**: keep synchronization/representation separate from added-value logic.
- **Model- and data-aware DT**: DTs require both, and must relate them.
- **Bidirectional interaction**: DTs observe and can command the AS through defined interfaces.

---

## Reference

- Jerôme Pfeiffer, Jingxi Zhang, Benoit Combemale, Judith Michael, Bernhard Rumpe, Manuel Wimmer, Andreas Wortmann.  
  *Towards a Unifying Reference Model for Digital Twins of Cyber-Physical Systems.* arXiv:2507.04871v1 (2025).  
  https://arxiv.org/html/2507.04871v1

---

## License
[LICENSE](LICENSE)

## Contact / Maintainers
- `...`
