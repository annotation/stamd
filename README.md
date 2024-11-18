<p align="center">
    <img src="https://github.com/annotation/stam/raw/master/logo.png" alt="stam logo" width="320" />
</p>

[![Project Status: WIP – Initial development is in progress, but there has not yet been a stable, usable release suitable for the public.](https://www.repostatus.org/badges/latest/wip.svg)](https://www.repostatus.org/#wip)
[![Crate](https://img.shields.io/crates/v/stamd.svg)](https://crates.io/crates/stamd)
[![Docs](https://docs.rs/stamd/badge.svg)](https://docs.rs/stamd/)
[![GitHub release](https://img.shields.io/github/release/annotation/stamd.svg)](https://GitHub.com/annotation/stamd/releases/)

# STAM Daemon

This is a webservice for working with stand-off annotation of text. It uses the
[STAM](https://annotation.github.io/stam) model, but can also serve [W3C Web
Annotations](https://www.w3.org/TR/annotation-model/) using a limited subset of
the [Web Annotation Protocol](https://www.w3.org/TR/annotation-protocol/), if
the underlying model supports it.

## Description & Features

A RESTful API is offered with several end-points. The full OpenAPI specification can be consulted
interactively at the `/swagger-ui/` endpoint once stamd is running.

stamd is a memory-backed service: models or annotation stores (i.e. annotations and the full
underlying texts), will be loaded into memory when first needed, and
automatically unloaded again when they haven't been used for a while. This
allows for very fast response times once a model is loaded, at the cost of
limited scalability with regard to the annotation store size and number of models serves
simultaneously.

Annotation stores can be queried via
[STAMQL](https://github.com/annotation/stam/tree/master/extensions/stam-query),
a powerful query language designed specifically for stand-off text annotation.
Queries are used not just to read information from the annotation stores, but
also to add/edit/delete information. The latter behaviour can be disallowed via
the `--read-only` parameter.

## Web API

Endpoints in this webservice return up to four different output formats, the format
is requested via regular HTTP *content negotation*:

* [**STAM JSON**](https://github.com/annotation/stam/?tab=readme-ov-file#stam-json) - `application/json` - This is STAM's canonical data format. It is returned by most of the endpoints.
* **plain text** - `text/plain` - Whenever output can be reduced to a plain text representation, this content type can be requested. It is also the default representation for the `/*/resources/` endpoints.
* **HTML** - `text/html` - This is only supported by the `/query/` and provides a complete HTML visualisation. In the query you can specify exactly what annotations to highlight. Read [further details here](https://github.com/annotation/stam-tools?tab=readme-ov-file#stam-view).
* [**W3C Web Annotations (JSON-LD)**](https://www.w3.org/TR/annotation-model/) - `application/ld+json` - This representation is allow on queries for annotations (`/query/`) and on the `/*/annotations/` endpoints. It returns the W3C Web Annotation representation in JSON-LD. The underlying STAM model must respect certain extra constraints, as formulated in the STAM specification, in order for this conversion to work.

The following endpoints are available, consult the `/swagger-ui/` endpoint for
a more formal and complete specification.

* `GET /`                  - Returns either a simple JSON list of all available annotation stores in this server, or a crude HTML form that allows you to interactively query any of the available stores.
* `GET /{store_id}/?query=`   - Runs a STAMQL query on an annotation store. This is the go-to endpoint that provides 90% of all functionality.
* `POST /query`               - Same as above but takes all paramters as form-encoded data via a POST request
* `POST /{store_id}`            - Create a new annotation store
* `GET /{store_id}/annotations` - Returns the public identifiers of all available annotations in the store.
* `GET /{store_id}/annotations/{annotation_id}` - Returns an annotation given its identifier.
* `GET /{store_id}/resources` - Returns the public identifiers of all available resources in the store.
* `GET /{store_id}/resources/{resource_id}` - Returns a resource given its identifier.
* `POST /{store_id}/resources/{resource_id}` - Create a new resource in a given store.
* `GET /{store_id}/resources/{resource_id}/{begin}/{end}` - Returns a text selection inside a resource. Offset are 0-indexed, unicode points, end is non inclusive.
* `GET /swagger-ui`       - Serves an interactive webinterface explaining the RESTful API specification.
* `GET /api-doc/openapi.json`   - Machine parseable OpenAPI specification.

## Installation

### From source

```
$ cargo install stamd
```

## Usage

Run `stamd` to start the webservice, see `stamd --help` for various parameters.
