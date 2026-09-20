"""Shortest safe lattice routes under the public SB-8 swept-move contract."""
import copy
import heapq
import math

from gantry_oracle import apply

AXES = ('x', 'z', 'y', 'yaw')


def shortest_yaw_delta(start, end):
    delta = (end - start + 180) % 360 - 180
    return 180 if delta == -180 else delta


def edge_cost(start, end):
    dy = end['y'] - start['y']
    return (1 + abs(end['x'] - start['x']) + abs(end['z'] - start['z'])
            + 3 * max(dy, 0) + max(-dy, 0)
            + abs(shortest_yaw_delta(start['yaw'], end['yaw'])) / 90)


def valid(request):
    if not isinstance(request, dict) or type(request.get('revision')) is not int:
        return False
    goal, lattice = request.get('goal'), request.get('lattice')
    if not isinstance(goal, dict) or not isinstance(lattice, dict):
        return False
    for axis in AXES:
        value, values = goal.get(axis), lattice.get(axis)
        if type(value) not in (int, float) or not math.isfinite(value):
            return False
        if not isinstance(values, list) or not values:
            return False
        if any(type(v) not in (int, float) or not math.isfinite(v) for v in values):
            return False
        if any(a >= b for a, b in zip(values, values[1:])):
            return False
    return True


def plan(scene, state, request):
    if not valid(request):
        return 400, {'error': 'invalid_plan'}
    axes = [request['lattice'][k] for k in AXES]
    if any(state['pose'][k] not in values or request['goal'][k] not in values
           for k, values in zip(AXES, axes)):
        return 400, {'error': 'invalid_plan'}
    if request['revision'] != state['revision']:
        return 409, {'error': 'stale_revision'}
    if state['held'] is None:
        return 422, {'error': 'not_holding'}
    start = tuple(values.index(state['pose'][k]) for k, values in zip(AXES, axes))
    goal = tuple(values.index(request['goal'][k]) for k, values in zip(AXES, axes))

    def pose(node):
        return {k: values[i] for k, values, i in zip(AXES, axes, node)}

    distances, previous = {start: 0}, {}
    queue = [(0, start)]
    while queue:
        cost, node = heapq.heappop(queue)
        if cost != distances[node]:
            continue
        if node == goal:
            route = [pose(node)]
            while node != start:
                node = previous[node]
                route.append(pose(node))
            return 200, {'revision': state['revision'], 'cost': cost, 'route': route[::-1]}
        source = pose(node)
        edge_state = copy.deepcopy(state)
        edge_state['pose'] = source
        held = next(b for b in edge_state['boxes'] if b['id'] == state['held'])
        held.update(source)
        for axis, values in enumerate(axes):
            neighbors = {node[axis] - 1, node[axis] + 1}
            if axis == 3:
                neighbors = {i % len(values) for i in neighbors}
            for index in sorted(neighbors):
                if not 0 <= index < len(values) or index == node[axis]:
                    continue
                target = node[:axis] + (index,) + node[axis + 1:]
                dest = pose(target)
                status, _ = apply(scene, edge_state, dict(
                    id='route-edge', revision=state['revision'], op='move', **dest))
                if status != 200:
                    continue
                candidate = cost + edge_cost(source, dest)
                if candidate < distances.get(target, math.inf):
                    distances[target], previous[target] = candidate, node
                    heapq.heappush(queue, (candidate, target))
    return 422, {'error': 'unreachable'}
