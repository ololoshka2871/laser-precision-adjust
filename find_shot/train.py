#!/usr/bin/bin python


import random
from simple_gen import GeneratedDataset, gen_stik_func
import torch.nn.functional as F
import torch.nn as nn
import torch
import numpy as np
from tqdm import tqdm


TRAIN_DATASET_SIZE = 10000
SIZE = 20


class Model(nn.Module):
    def __init__(self, input_size: int, output_size: int, hidden_size: int = 2):
        super(Model, self).__init__()
        self.l1 = nn.Linear(input_size, output_size)
        self.relu = nn.ReLU()
        self.lh = []
        for _ in range(hidden_size):
            self.lh.append(nn.Linear(output_size, output_size))
        self.le = nn.Linear(output_size, output_size)

    def forward(self, x):
        x = self.l1(x)
        x = self.relu(x)
        for l in self.lh:
            x = l(x)
            x = self.relu(x)
        x = self.le(x)
        x = self.relu(x)
        return x


def acuracy(pred, label) -> float:
    ress = pred.detach().numpy().argmax(axis=1) 
    labels = label.detach().numpy().argmax(axis=1)
    
    zip = np.column_stack((ress, labels))
    equals = list(map(lambda t: 1.0 if t[0] == t[1] else 0.0, zip))
    
    return sum(equals) / len(equals)


def main():
    def noise_func(_y): return random.randrange(-1, 3) / 200.0

    random_styik = gen_stik_func(lambda y1, y2: (
        abs(y1-y2) + abs(noise_func(abs(y1-y2)))) * 10.0)

    if torch.cuda.is_available() and False:
        print("Using CUDA")
        torch.set_default_tensor_type(torch.cuda.FloatTensor)
        torch_device = torch.device('cuda')
    else:
        print("Using CPU")
        torch_device = torch.device('cpu')

    dataset_train = GeneratedDataset(
        noise_func, random_styik, SIZE, TRAIN_DATASET_SIZE)
    data_loader_train = torch.utils.data.DataLoader(
        dataset_train, batch_size=16)

    model_train = Model(SIZE, SIZE, hidden_size=2).to(torch_device)

    loss_fn = nn.CrossEntropyLoss()
    optimizer = torch.optim.Adam(model_train.parameters(), lr=1e-3)

    epohs = 100

    for ep in range(epohs):
        loss_epoh = 0.0
        acuracy_epoh = 0.0
        for item in (pbar := tqdm(iter(data_loader_train))):
            data = item['val'].to(torch_device)
            result = item['is_fired'].to(torch_device)

            pred = model_train(data)
            loss = loss_fn(pred, result)

            loss_item = loss.item()
            loss_epoh += loss_item

            optimizer.zero_grad(True)
            loss.backward()
            optimizer.step()

            a = acuracy(pred.to('cpu'), result.to('cpu'))
            acuracy_epoh += a

            # pbar.set_description(f"Epoh {ep} loss {loss_item:.4f}\tAccuracy: {a:.3f}")

        print(
            f"Epoh {ep} loss {loss_epoh / TRAIN_DATASET_SIZE:.4f}\tAccuracy: {acuracy_epoh / TRAIN_DATASET_SIZE:.3f}")


if __name__ == "__main__":
    main()
