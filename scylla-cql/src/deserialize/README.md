Framework for deserialization of data returned by database queries.

Deserialization is based on two traits:

- A type that implements `DeserializeValue<'frame, 'metadata>` can be deserialized
  from a single _CQL value_ - i.e. an element of a row in the query result,
- A type that implements `DeserializeRow<'frame, 'metadata>` can be deserialized
  from a single _row_ of a query result.

Those traits are quite similar to each other, both in the idea behind them
and the interface that they expose.

It's important to understand what is a _deserialized type_. It's not just
an implementor of `Deserialize{Value, Row}`; there are some implementors of
`Deserialize{Value, Row}` who are not yet final types, but **partially**
deserialized types that support further deserialization - _type
deserializers_, such as `ListlikeIterator`, `UdtIterator` or `ColumnIterator`.

# Lifetime parameters

- `'frame` is the lifetime of the frame. Any deserialized type that is going to borrow
  from the frame must have its lifetime bound by `'frame`.
- `'metadata` is the lifetime of the result metadata. As result metadata is only needed
  for the very deserialization process and the **final** deserialized types (i.e. those
  that are not going to deserialize anything else, opposite of e.g. `MapIterator`) can
  later live independently of the metadata, this is different from `'frame`.

_Type deserializers_, as they still need to deserialize some type, are naturally bound
by 'metadata lifetime. However, final types are completely deserialized, so they should
not be bound by 'metadata - only by 'frame.

Rationale:
`DeserializeValue` requires two types of data in order to perform
deserialization:
1) a reference to the CQL frame (a FrameSlice),
2) the type of the column being deserialized, being part of the
   ResultMetadata.

Similarly, `DeserializeRow` requires two types of data in order to
perform deserialization:
1) a reference to the CQL frame (a FrameSlice),
2) a slice of specifications of all columns in the row, being part of
   the ResultMetadata.

When deserializing owned types, both the frame and the metadata can have
any lifetime and it's not important. When deserializing borrowed types,
however, they borrow from the frame, so their lifetime must necessarily
be bound by the lifetime of the frame. Metadata is only needed for the
deserialization, so its lifetime does not abstractly bound the
deserialized value. Not to unnecessarily shorten the deserialized
values' lifetime to the metadata's lifetime (due to unification of
metadata's and frame's lifetime in value deserializers), a separate
lifetime parameter is introduced for result metadata: `'metadata`.

# `type_check` and `deserialize`

The deserialization process is divided into two parts: type checking and
actual deserialization, represented by `DeserializeValue`/`DeserializeRow`'s
methods called `type_check` and `deserialize`.

The `deserialize` method can assume that `type_check` was called before, so
it doesn't have to verify the type again. This can be a performance gain
when deserializing query results with multiple rows: as each row in a result
has the same type, it is only necessary to call `type_check` once for the
whole result and then `deserialize` for each row.

Note that `deserialize` is not an `unsafe` method - although you can be
sure that the driver will call `type_check` before `deserialize`, you
shouldn't do unsafe things based on this assumption.

# Data ownership

Some CQL types can be easily consumed while still partially serialized.
For example, types like `blob` or `text` can be just represented with
`&[u8]` and `&str` that just point to a part of the serialized response.
This is more efficient than using `Vec<u8>` or `String` because it avoids
an allocation and a copy, however it is less convenient because those types
are bound with a lifetime.

The framework supports types that refer to the serialized response's memory
in three different ways:

## Owned types

Some types don't borrow anything and fully own their data, e.g. `i32` or
`String`. They aren't constrained by any lifetime and should implement
the respective trait for _all_ lifetimes, i.e.:

```rust
# use scylla_cql::frame::response::result::{NativeType, ColumnType};
# use scylla_cql::deserialize::{DeserializationError, FrameSlice, TypeCheckError};
# use scylla_cql::deserialize::value::DeserializeValue;
use thiserror::Error;
struct MyVec(Vec<u8>);
#[derive(Debug, Error)]
enum MyDeserError {
    #[error("Expected bytes")]
    ExpectedBytes,
    #[error("Expected non-null")]
    ExpectedNonNull,
}
impl<'frame, 'metadata> DeserializeValue<'frame, 'metadata> for MyVec {
    fn type_check(typ: &ColumnType) -> Result<(), TypeCheckError> {
        if let ColumnType::Native(NativeType::Blob) = typ {
            return Ok(());
        }
        Err(TypeCheckError::new(MyDeserError::ExpectedBytes))
    }

    fn deserialize(
        _typ: &'metadata ColumnType<'metadata>,
        v: Option<FrameSlice<'frame>>,
    ) -> Result<Self, DeserializationError> {
        v.ok_or_else(|| DeserializationError::new(MyDeserError::ExpectedNonNull))
            .map(|v| Self(v.as_slice().to_vec()))
    }
}
```

## Borrowing types

Some types do not fully contain their data but rather will point to some
bytes in the serialized response, e.g. `&str` or `&[u8]`. Those types will
usually contain a lifetime in their definition. In order to properly
implement `DeserializeValue` or `DeserializeRow` for such a type, the `impl`
should still have a generic lifetime parameter, but the lifetimes from the
type definition should be constrained with the generic lifetime parameter.
For example:

```rust
# use scylla_cql::frame::response::result::{NativeType, ColumnType};
# use scylla_cql::deserialize::{DeserializationError, FrameSlice, TypeCheckError};
# use scylla_cql::deserialize::value::DeserializeValue;
use thiserror::Error;
struct MySlice<'a>(&'a [u8]);
#[derive(Debug, Error)]
enum MyDeserError {
    #[error("Expected bytes")]
    ExpectedBytes,
    #[error("Expected non-null")]
    ExpectedNonNull,
}
impl<'a, 'frame, 'metadata> DeserializeValue<'frame, 'metadata> for MySlice<'a>
where
    'frame: 'a,
{
    fn type_check(typ: &ColumnType) -> Result<(), TypeCheckError> {
        if let ColumnType::Native(NativeType::Blob) = typ {
            return Ok(());
        }
        Err(TypeCheckError::new(MyDeserError::ExpectedBytes))
    }

    fn deserialize(
        _typ: &'metadata ColumnType<'metadata>,
        v: Option<FrameSlice<'frame>>,
    ) -> Result<Self, DeserializationError> {
        v.ok_or_else(|| DeserializationError::new(MyDeserError::ExpectedNonNull))
            .map(|v| Self(v.as_slice()))
    }
}
```

## Reference-counted types

Internally, the driver uses the `bytes::Bytes` type to keep the contents
of the serialized response. It supports creating derived `Bytes` objects
which point to a subslice but keep the whole, original `Bytes` object alive.

During deserialization, a type can obtain a `Bytes` subslice that points
to the serialized value. This approach combines advantages of the previous
two approaches - creating a derived `Bytes` object can be cheaper than
allocation and a copy (it supports `Arc`-like semantics) and the `Bytes`
type is not constrained by a lifetime. However, you should be aware that
the subslice will keep the whole `Bytes` object that holds the frame alive.
It is not recommended to use this approach for long-living objects because
it can introduce space leaks.

Example:

```rust
# use scylla_cql::frame::response::result::{NativeType, ColumnType};
# use scylla_cql::deserialize::{DeserializationError, FrameSlice, TypeCheckError};
# use scylla_cql::deserialize::value::DeserializeValue;
# use bytes::Bytes;
use thiserror::Error;
struct MyBytes(Bytes);
#[derive(Debug, Error)]
enum MyDeserError {
    #[error("Expected bytes")]
    ExpectedBytes,
    #[error("Expected non-null")]
    ExpectedNonNull,
}
impl<'frame, 'metadata> DeserializeValue<'frame, 'metadata> for MyBytes {
    fn type_check(typ: &ColumnType) -> Result<(), TypeCheckError> {
        if let ColumnType::Native(NativeType::Blob) = typ {
            return Ok(());
        }
        Err(TypeCheckError::new(MyDeserError::ExpectedBytes))
    }

    fn deserialize(
        _typ: &'metadata ColumnType<'metadata>,
        v: Option<FrameSlice<'frame>>,
    ) -> Result<Self, DeserializationError> {
        v.ok_or_else(|| DeserializationError::new(MyDeserError::ExpectedNonNull))
            .map(|v| Self(v.to_bytes()))
    }
}
```
