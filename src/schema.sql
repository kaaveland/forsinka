-- The schema is completely re-installed when the app boots, regardless of whether there's data there or not.
-- The tables are recreated, empty. Therefore the app must populate with some initial data before it's done booting.
-- This pattern does not work well with persistent data files, so it's more suitable for the in-memory database mode.

-- https://data.entur.no/dataset/national_stop_registry see quays_last_version
create or replace table quays (
    id varchar not null,
    version bigint,
    publicCode varchar,
    name varchar,
    shortName varchar,
    description varchar,
    location_longitude double,
    location_latitude double,
    -- stops.id, but FK not supported
    stopPlaceRef varchar
);

-- https://data.entur.no/dataset/national_stop_registry see stop_places_last_version
create or replace table stops (
    -- quays.stopPlaceRef, but FK not supported
    id varchar not null,
    version bigint,
    publicCode varchar,
    transportMode varchar,
    name varchar,
    shortName varchar,
    description varchar,
    location_longitude double,
    location_latitude double,
    topographicPlaceRef struct("version" bigint, "ref" varchar),
    alternativeNames struct("name" varchar)[],
    tariffZoneRefs struct("version" bigint, "ref" varchar)[],
    fareZoneRefs struct("version" bigint, "ref" varchar)[],
    validBetween struct("toDate" timestamp with time zone, "fromDate" timestamp with time zone),
    parentRef struct("ref" varchar, "version" bigint)
);
