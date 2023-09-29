{% macro table_row(number, start, end, ppm, ok) -%}
|{{number}}|{{start}}| {{end}}        |{{ppm}}|{{ok}}|               |     |       |
{% endmacro -%}

## Отчет о точной настройке партии {{part_id}}

### Настройки
- __Диопазон__: {{ freq_target }} ± {{ ppm }} ppm: {{ f_min }} - {{ f_max }} Гц.
- __Поправка частотомера__: {{ work_offset_hz }} Гц.

### Резонаторы
| №   | Частота исх. | Частота настр. | ppm    | ok? | Частота корп. | ppm   | Годн? |
| --- | ------------ | -------------- | ------ | --- |-------------- | ----- | ----- |
{% for rez in rezonators -%}
{{ table_row(loop.index, rez.start, rez.end, rez.ppm, rez.ok) }}
{% endfor %}