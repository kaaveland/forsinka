with stopdata as (
    from quays q join stops s on q.stopPlaceRef = s.id
  select
    q.id as stop_point_ref,
    coalesce(s.name, q.name) as stop_name,
    coalesce(q.location_latitude, s.location_latitude) as lat,
    coalesce(q.location_longitude, s.location_longitude) as lon
), last_rec as (
    from recorded_call join stopdata using(stop_point_ref)
  select distinct on(vehicle_journey_id)
    vehicle_journey_id,
    coalesce(aimed_arrival_time, aimed_departure_time) as aimed_time,
    coalesce(actual_arrival_time, actual_departure_time) as actual_time,
    actual_time - aimed_time as delay,
    stop_name,
    lat,
    lon
  order by "order" desc
), next_est as (
    from estimated_call join stopdata using(stop_point_ref)
  select distinct on(vehicle_journey_id)
    vehicle_journey_id,
    coalesce(aimed_arrival_time, aimed_departure_time) as next_aimed_time,
    stop_name as next_stop_name,
    lat as next_lat,
    lon as next_lon
  order by "order" asc
), pass_by_stop as (
  from estimated_call join stopdata using(stop_point_ref)
  select vehicle_journey_id
  where stopdata.stop_name = $1 -- 'Billingstad stasjon'
)
    from last_rec lr left join next_est ne using(vehicle_journey_id)
  join vehicle_journey vj using(vehicle_journey_id)
  join pass_by_stop using(vehicle_journey_id)
select
    vj.vehicle_journey_id,
    vj.line_ref,
    lr.stop_name,
    lr.aimed_time,
    lr.actual_time,
    (extract (epoch from lr.delay)) :: int4 as last_delay,
    ne.next_stop_name,
    ne.next_aimed_time
where started and not finished -- and vj.data_source in ('BNR', 'VYG', 'SJN', 'FLY')
order by last_delay desc
;