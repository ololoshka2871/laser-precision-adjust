import json


def load_all_series(file: str) -> list:
    series = []

    with open(file, "r") as f:
        current_seriae = []
        prev_channel = None
        line_number = 0

        # Read each line
        for line in f:
            line_number += 1

            # Read each JSON object
            obj = json.loads(line)

            if obj is None:
                continue

            # Get channel name
            channel_name = obj["channel"]

            # If channel name is not in seriaes, add it
            if (prev_channel != None) and (channel_name != prev_channel):
                series.append(current_seriae)
                current_seriae = [obj["f"]]
                prev_channel = channel_name
            elif prev_channel == None:
                current_seriae = [obj["f"]]
                prev_channel = channel_name
            else:
                current_seriae.append(obj["f"])
    
    return series


def load_serie(file: str, serie_n: int, max_size: int = 100) -> list:
    series = []

    with open(file, "r") as f:
        current_seriae = []
        prev_channel = None
        line_number = 0

        # Read each line
        for line in f:
            line_number += 1

            # Read each JSON object
            obj = json.loads(line)

            if obj is None:
                continue

            # Get channel name
            channel_name = obj["channel"]

            # If channel name is not in seriaes, add it
            if ((prev_channel != None) and (channel_name != prev_channel)) \
                or len(current_seriae) == max_size:
                
                if len(current_seriae) == max_size:
                    series.append(current_seriae)

                current_seriae = [obj["f"]]
                prev_channel = channel_name
            elif prev_channel == None:
                current_seriae = [obj["f"]]
                prev_channel = channel_name
            else:
                current_seriae.append(obj["f"])

    return series[serie_n]
