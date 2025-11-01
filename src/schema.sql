-- The schema is completely re-installed when the app boots, regardless of whether there's data there or not.
-- The tables are recreated, empty. Therefore the app must populate with some initial data before it's done booting.
-- This pattern does not work well with persistent data files, so it's more suitable for the in-memory database mode.
install spatial;
load spatial;

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

create or replace table vehicle_journey(
    -- probably block_ref/dated_vehicle_journey_ref
    id varchar not null,
    -- ATB, RUT, ...
    data_source varchar not null,
    -- last imported time - we can have multiple imports, and should only show one!
    recorded_at_time timestamp with time zone,
    -- entire journey cancelled
    cancellation bool,
    -- replaced another vehicle_journey (eg. bus for train)
    extra_journey bool,
    line_ref varchar,
    direction_ref varchar,
    -- keys in stops.id _or_ quays.id to fetch name
    destination_ref varchar,
    origin_ref varchar
);

-- arrivals that already happened, key is vehicle_journey_id + data_source + order + recorded_at_time
-- but we should only show the row with the highest recorded_at_time
create or replace table recorded_call(
    -- the first columns are for identification / connecting to vehicle journey
    -- probably block_ref/dated_vehicle_journey_ref
    vehicle_journey_id varchar not null,
    -- ATB, RUT, ...
    data_source varchar not null,
    -- last imported time - we can have multiple imports, and should only show one!
    recorded_at_time timestamp with time zone,
    -- order of this stop within the route, generally if row a has order < row b, then row a has aimed_arrival_time <= row b
    -- for one vehicle_journey, the next destination has order = max(recorded_call.order) + 1
    "order" smallint not null,

    -- schedule - note that if order = 1, we do not have arrival (start point)
    -- and at order = max(order) we do not have departure (end point)
    aimed_arrival_time timestamp with time zone,
    aimed_departure_time timestamp with time zone,
    -- actual
    actual_arrival_time timestamp with time zone,
    actual_departure_time timestamp with time zone,
    -- this particular stop has been cancelled for the journey
    cancellation bool,
    -- connects to _either_ quays.id or stops.id, where we should fetch name and coordinates
    stop_point_ref varchar not null
);

-- arrivals yet to take place, key is vehicle_journey_id + data_source + order + recorded_at_time
-- but we should only show the row with the highest recorded_at_time
create or replace table estimated_call(
    -- the first columns are for identification / connecting to vehicle journey
    -- probably block_ref/dated_vehicle_journey_ref
    vehicle_journey_id varchar not null,
    -- ATB, RUT, ...
    data_source varchar not null,
    -- last imported time - we can have multiple imports, and should only show one!
    recorded_at_time timestamp with time zone,
    -- order of this stop within the route, generally if row a has order < row b, then row a has aimed_arrival_time <= row b
    -- for one vehicle_journey, the next destination has order = max(recorded_call.order) + 1
    "order" smallint not null,

    -- schedule - note that if order = 1, we do not have arrival (start point)
    -- and at order = max(order) we do not have departure (end point)
    aimed_arrival_time timestamp with time zone,
    aimed_departure_time timestamp with time zone,
    -- last real time estimate
    expected_arrival_time timestamp with time zone,
    expected_departure_time timestamp with time zone,
    -- this particular stop has been cancelled for the journey
    cancellation bool,
    -- connects to _either_ quays.id or stops.id, where we should fetch name and coordinates
    stop_point_ref varchar not null
);
