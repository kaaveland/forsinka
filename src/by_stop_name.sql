with last_rec as (
  from recorded_call join stopdata using(stop_point_ref)
  select distinct on(vehicle_journey_id)
    vehicle_journey_id,
    coalesce(aimed_arrival_time, aimed_departure_time) as aimed_time,
    coalesce(actual_arrival_time, actual_departure_time) as actual_time,
    actual_time - aimed_time as delay,
    stopdata.name as stop_name,
    lat,
    lon,
    data_source
  order by "order" desc
), next_est as (
  from estimated_call join stopdata using(stop_point_ref)
  select distinct on(vehicle_journey_id)
    vehicle_journey_id,
    coalesce(aimed_arrival_time, aimed_departure_time) as next_aimed_time,
    stopdata.name as next_stop_name,
    lat as next_lat,
    lon as next_lon,
    data_source
  order by "order" asc
), pass_by_stop as (
  from estimated_call join stopdata using(stop_point_ref)
  select vehicle_journey_id, data_source
  where stopdata.name = $1 -- 'Billingstad stasjon'
)
from last_rec lr left join next_est ne using(vehicle_journey_id, data_source)
  join vehicle_journey vj using(vehicle_journey_id, data_source)
  join pass_by_stop using(vehicle_journey_id, data_source)
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