from pyparsing import Iterator

from denoize import create_denoized_spline, build_box_plot
from shot_detector import detect_shots


MIN_POINTS = 15
ZAPAS = 2


def fragment_iterator(iterator: Iterator[float], smooth: float = 1.0, max_points: int = 100) -> list[(int, list[float])]:
    '''
    Iterator for series of fragments
    '''

    block = []
    found_point_n = None
    found_fragment_smooth = None
    points_counter = 0
    for point in iterator:
        block.append(point)
        points_counter += 1
        if len(block) < MIN_POINTS:
            continue
        
        print(f"Окно поиска {points_counter - len(block)}:{points_counter}")
        if len(block) > max_points:
            block = block[ZAPAS:]

        if found_point_n is not None:
            print(f"Окно просмотра {points_counter - len(block) + ZAPAS + 1}:{points_counter}")
            an_block = block[ZAPAS + 1:]
        else:
            print(f"Окно просмотра {points_counter - len(block)}:{points_counter}")
            an_block = block
        dns, _ = create_denoized_spline(an_block, smooth)

        smooth_block = [dns.Y(x) for x in range(len(an_block) - ZAPAS)]
        derivative_block = [dns.dY(x) for x in range(len(an_block) - ZAPAS)]
        derivative_bxplt = build_box_plot(derivative_block)
        if abs(derivative_bxplt['lower_bound'] - derivative_bxplt['upper_bound']) < 0.1:
            print(f"Слишком маленький размах производной ({abs(derivative_bxplt['lower_bound'] - derivative_bxplt['upper_bound'])})")
            first_shot = None
        else:
            print(f"Размах производной {abs(derivative_bxplt['lower_bound'] - derivative_bxplt['upper_bound'])}")
            try:
                first_shot = next(detect_shots(derivative_block,
                                               derivative_bxplt['lower_bound'],
                                               derivative_bxplt['upper_bound']))
                print(f"Найден выстрел на {first_shot} ({points_counter - len(block) + first_shot})")
            except StopIteration:
                print("Выстрел не найден")
                first_shot = None

        if (first_shot is None) or (first_shot < ZAPAS):
            # too near from start
            print("Выстрел не найден или слишком близко к началу, следующий цыкл..")
            if found_point_n is not None:
                print(f"Превышена длина фрагмента, возвращаем {found_point_n}:{len(found_fragment_smooth)}")
                yield (found_point_n, found_fragment_smooth)
                found_point_n = None
                block = block[ZAPAS + 1:]
            continue

        # found correct shot
        if not found_point_n:
            # first candidate found
            found_point_n = points_counter - len(an_block)
            found_fragment_smooth = smooth_block
            print(f"Первый выстрел найден {found_point_n}:len={len(found_fragment_smooth)}")
            # Отбрасываем все точки раньше чем найденный выстрел и еще ZAPAS оставляем в запасе
            block = block[first_shot - ZAPAS:]
        else:
            print(f"Второй выстрел найден фрагмент длиной {len(smooth_block)}")
            yield (found_point_n, found_fragment_smooth[:-ZAPAS])
            # тут ZAPAS уже вычтен
            block = block[first_shot:]

    # end for
    print(f"Остаток {len(block)}")
    if found_point_n is not None:
        yield (found_point_n, found_fragment_smooth)
